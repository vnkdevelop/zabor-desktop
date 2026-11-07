use crate::audio_settings::AudioSettings;
use crate::media::PeerLifetime;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    Arc, Mutex,
};

pub struct AudioControls {
    pub settings: Mutex<AudioSettings>,
    state: AtomicU32,
    pub mic_db: AtomicU32,
    pub speaking: AtomicBool,
    pub processor_failed: AtomicBool,
    pub settings_revision: AtomicU64,
    pub capture_epoch: AtomicU64,
    pub playback_epoch: AtomicU64,
    pub render_delay_ms: AtomicU32,
}

impl AudioControls {
    pub fn new(settings: AudioSettings) -> Self {
        Self {
            settings: Mutex::new(settings),
            state: AtomicU32::new(0),
            mic_db: AtomicU32::new((-100.0_f32).to_bits()),
            speaking: AtomicBool::new(false),
            processor_failed: AtomicBool::new(false),
            settings_revision: AtomicU64::new(0),
            capture_epoch: AtomicU64::new(0),
            playback_epoch: AtomicU64::new(0),
            render_delay_ms: AtomicU32::new(0),
        }
    }
    pub fn effectively_muted(&self) -> bool {
        self.state.load(Ordering::Acquire) != 0
    }
    pub fn effectively_deafened(&self) -> bool {
        self.state.load(Ordering::Acquire) & 0b1010 != 0
    }
    pub fn set_user_state(&self, muted: bool, deafened: bool) {
        self.set_state(u32::from(muted) | (u32::from(deafened) << 1), 0b0011);
    }
    pub fn set_server_state(&self, muted: bool, deafened: bool) {
        self.set_state((u32::from(muted) << 2) | (u32::from(deafened) << 3), 0b1100);
    }
    fn set_state(&self, bits: u32, mask: u32) {
        let mut state = self.state.load(Ordering::Acquire);
        let previous = loop {
            match self.state.compare_exchange_weak(
                state,
                (state & !mask) | bits,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(previous) => break previous,
                Err(current) => state = current,
            }
        };
        let changed = (previous & mask) ^ bits;
        if changed != 0 {
            self.capture_epoch.fetch_add(1, Ordering::AcqRel);
        }
        if changed & 0b1010 != 0 {
            self.playback_epoch.fetch_add(1, Ordering::AcqRel);
        }
        if self.effectively_muted() {
            self.speaking.store(false, Ordering::Release);
        }
    }
    pub fn user_state(&self) -> (bool, bool) {
        let state = self.state.load(Ordering::Acquire);
        (state & 1 != 0, state & 2 != 0)
    }
    pub fn server_state(&self) -> (bool, bool) {
        let state = self.state.load(Ordering::Acquire);
        (state & 4 != 0, state & 8 != 0)
    }
    pub fn apply_settings(&self, settings: AudioSettings) -> bool {
        if let Ok(mut current) = self.settings.lock() {
            if *current != settings {
                *current = settings;
                self.settings_revision.fetch_add(1, Ordering::Release);
            }
            true
        } else {
            false
        }
    }
}

pub struct AudioLane {
    queue: Mutex<VecDeque<f32>>,
    active: AtomicBool,
    lifetime: PeerLifetime,
    stream: bool,
    gain: AtomicU32,
    peer: String,
    controls: Arc<AudioControls>,
    playback_epoch: AtomicU64,
}

impl AudioLane {
    fn new(
        peer: String,
        lifetime: PeerLifetime,
        stream: bool,
        gain: f32,
        controls: Arc<AudioControls>,
    ) -> Self {
        Self {
            queue: Mutex::new(VecDeque::with_capacity(9600)),
            active: AtomicBool::new(true),
            lifetime,
            stream,
            gain: AtomicU32::new(gain.to_bits()),
            peer,
            playback_epoch: AtomicU64::new(controls.playback_epoch.load(Ordering::Acquire)),
            controls,
        }
    }
    pub fn push(&self, samples: &[i16]) {
        if !self.active.load(Ordering::Acquire) || !self.lifetime.is_alive() || samples.len() > 9600
        {
            return;
        }
        if let Ok(mut queue) = self.queue.lock() {
            let epoch = self.controls.playback_epoch.load(Ordering::Acquire);
            if self.playback_epoch.swap(epoch, Ordering::AcqRel) != epoch {
                queue.clear();
            }
            if !self.is_active() || self.controls.effectively_deafened() {
                queue.clear();
                return;
            }
            let extra = queue
                .len()
                .saturating_add(samples.len())
                .saturating_sub(9600);
            queue.drain(..extra);
            queue.extend(samples.iter().map(|sample| f32::from(*sample) / 32768.0));
        }
    }
    pub fn stop(&self) {
        self.active.store(false, Ordering::Release);
        if let Ok(mut queue) = self.queue.lock() {
            queue.clear();
        }
    }
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire) && self.lifetime.is_alive()
    }
}

pub struct AudioMixer {
    lanes: Mutex<BTreeMap<String, Arc<AudioLane>>>,
    pub reverse: Mutex<VecDeque<i16>>,
    controls: Arc<AudioControls>,
    focused_stream: Mutex<Option<String>>,
}

impl AudioMixer {
    pub fn new(controls: Arc<AudioControls>) -> Self {
        Self {
            lanes: Mutex::new(BTreeMap::new()),
            reverse: Mutex::new(VecDeque::with_capacity(4800)),
            controls,
            focused_stream: Mutex::new(None),
        }
    }
    pub fn add(
        &self,
        id: String,
        peer: &str,
        lifetime: PeerLifetime,
        stream: bool,
    ) -> Option<Arc<AudioLane>> {
        let settings = self.controls.settings.lock().ok()?;
        let gain = settings.peer_gain(peer);
        let mut lanes = self.lanes.lock().ok()?;
        if lanes.len() >= 128 && !lanes.contains_key(&id) {
            return None;
        }
        let lane = Arc::new(AudioLane::new(
            peer.into(),
            lifetime,
            stream,
            gain,
            self.controls.clone(),
        ));
        if let Some(previous) = lanes.insert(id, lane.clone()) {
            previous.stop();
        }
        drop(settings);
        Some(lane)
    }
    pub fn remove(&self, id: &str) {
        if let Ok(mut lanes) = self.lanes.lock() {
            if let Some(lane) = lanes.remove(id) {
                lane.stop();
            }
        }
    }
    pub fn remove_lane(&self, id: &str, expected: &Arc<AudioLane>) {
        if let Ok(mut lanes) = self.lanes.lock() {
            if lanes
                .get(id)
                .is_some_and(|lane| Arc::ptr_eq(lane, expected))
            {
                if let Some(lane) = lanes.remove(id) {
                    lane.stop();
                }
            }
        }
        expected.stop();
    }
    pub fn focus(&self, peer: Option<String>) {
        if let Ok(mut focused) = self.focused_stream.lock() {
            if *focused != peer {
                if let Ok(lanes) = self.lanes.lock() {
                    for lane in lanes.values().filter(|lane| lane.stream) {
                        if let Ok(mut samples) = lane.queue.lock() {
                            samples.clear();
                        }
                    }
                }
            }
            *focused = peer;
        }
    }
    pub fn update_gains(&self, settings: &AudioSettings) {
        if let Ok(lanes) = self.lanes.lock() {
            for lane in lanes.values() {
                lane.gain
                    .store(settings.peer_gain(&lane.peer).to_bits(), Ordering::Release);
            }
        }
    }
    pub fn render(&self, stereo: &mut [f32]) {
        self.render_pcm(stereo);
        self.played(stereo);
    }
    pub fn render_pcm(&self, stereo: &mut [f32]) {
        stereo.fill(0.0);
        let epoch = self.controls.playback_epoch.load(Ordering::Acquire);
        let muted = self.controls.effectively_deafened();
        let focused = self.focused_stream.try_lock().ok();
        if let Ok(mut lanes) = self.lanes.try_lock() {
            lanes.retain(|_, lane| lane.is_active());
            for lane in lanes.values() {
                let audible = !muted
                    && (!lane.stream
                        || focused.as_ref().and_then(|peer| peer.as_deref()) == Some(&lane.peer));
                if let Ok(mut samples) = lane.queue.try_lock() {
                    let epoch = self.controls.playback_epoch.load(Ordering::Acquire);
                    if lane.playback_epoch.swap(epoch, Ordering::AcqRel) != epoch {
                        samples.clear();
                    }
                    if !audible || !lane.is_active() {
                        samples.clear();
                        continue;
                    }
                    let gain = f32::from_bits(lane.gain.load(Ordering::Acquire));
                    for frame in stereo.as_chunks_mut::<2>().0 {
                        let sample = samples.pop_front().unwrap_or(0.0) * gain;
                        frame[0] += sample;
                        frame[1] += sample;
                    }
                }
            }
        }
        if self.controls.effectively_deafened()
            || self.controls.playback_epoch.load(Ordering::Acquire) != epoch
        {
            stereo.fill(0.0);
        }
        for sample in stereo.iter_mut() {
            *sample = sample.clamp(-0.84139514, 0.84139514);
        }
    }
    pub fn played(&self, stereo: &[f32]) {
        if let Ok(mut reverse) = self.reverse.try_lock() {
            let count = stereo.len() / 2;
            let extra = reverse.len().saturating_add(count).saturating_sub(4800);
            if extra <= reverse.len() {
                reverse.drain(..extra);
            } else {
                reverse.clear();
            }
            let frames = stereo.as_chunks::<2>().0;
            reverse.extend(
                frames[frames.len().saturating_sub(4800)..]
                    .iter()
                    .map(|frame| ((frame[0] + frame[1]) * 0.5 * 32767.0) as i16),
            );
        }
    }
    pub fn clear(&self) {
        if let Ok(mut lanes) = self.lanes.lock() {
            for lane in lanes.values() {
                lane.stop();
            }
            lanes.clear();
        }
        if let Ok(mut reverse) = self.reverse.lock() {
            reverse.clear();
        }
    }
}

impl Drop for AudioMixer {
    fn drop(&mut self) {
        if let Ok(lanes) = self.lanes.lock() {
            for lane in lanes.values() {
                lane.stop();
            }
        }
    }
}
