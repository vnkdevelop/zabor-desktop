use crate::audio_settings::AudioSettings;
use crate::media::PeerLifetime;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Mutex,
};

pub struct AudioControls {
    pub settings: Mutex<AudioSettings>,
    pub muted: AtomicBool,
    pub deafened: AtomicBool,
    pub server_muted: AtomicBool,
    pub server_deafened: AtomicBool,
    pub mic_db: AtomicU32,
    pub speaking: AtomicBool,
    pub processor_failed: AtomicBool,
}

impl AudioControls {
    pub fn new(settings: AudioSettings) -> Self {
        Self {
            settings: Mutex::new(settings),
            muted: AtomicBool::new(false),
            deafened: AtomicBool::new(false),
            server_muted: AtomicBool::new(false),
            server_deafened: AtomicBool::new(false),
            mic_db: AtomicU32::new((-100.0_f32).to_bits()),
            speaking: AtomicBool::new(false),
            processor_failed: AtomicBool::new(false),
        }
    }
    pub fn effectively_muted(&self) -> bool {
        self.muted.load(Ordering::Acquire)
            || self.deafened.load(Ordering::Acquire)
            || self.server_muted.load(Ordering::Acquire)
            || self.server_deafened.load(Ordering::Acquire)
    }
    pub fn effectively_deafened(&self) -> bool {
        self.deafened.load(Ordering::Acquire) || self.server_deafened.load(Ordering::Acquire)
    }
    pub fn set_user_state(&self, muted: bool, deafened: bool) {
        self.muted.store(muted, Ordering::Release);
        self.deafened.store(deafened, Ordering::Release);
        if self.effectively_muted() {
            self.speaking.store(false, Ordering::Release);
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
}

impl AudioLane {
    fn new(peer: String, lifetime: PeerLifetime, stream: bool, gain: f32) -> Self {
        Self {
            queue: Mutex::new(VecDeque::with_capacity(9600)),
            active: AtomicBool::new(true),
            lifetime,
            stream,
            gain: AtomicU32::new(gain.to_bits()),
            peer,
        }
    }
    pub fn push(&self, samples: &[i16]) {
        if !self.active.load(Ordering::Acquire) || !self.lifetime.is_alive() || samples.len() > 9600
        {
            return;
        }
        if let Ok(mut queue) = self.queue.lock() {
            if !self.is_active() {
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
    fn is_active(&self) -> bool {
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
        let gain = self.controls.settings.lock().ok()?.peer_gain(peer);
        let mut lanes = self.lanes.lock().ok()?;
        if lanes.len() >= 128 && !lanes.contains_key(&id) {
            return None;
        }
        let lane = Arc::new(AudioLane::new(peer.into(), lifetime, stream, gain));
        if let Some(previous) = lanes.insert(id, lane.clone()) {
            previous.stop();
        }
        Some(lane)
    }
    pub fn remove(&self, id: &str) {
        if let Ok(mut lanes) = self.lanes.lock() {
            if let Some(lane) = lanes.remove(id) {
                lane.stop();
            }
        }
    }
    pub fn focus(&self, peer: Option<String>) {
        if let Ok(mut focused) = self.focused_stream.lock() {
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
        stereo.fill(0.0);
        let muted = self.controls.effectively_deafened();
        let focused = self.focused_stream.try_lock().ok();
        if let Ok(mut lanes) = self.lanes.try_lock() {
            lanes.retain(|_, lane| lane.is_active());
            for lane in lanes.values() {
                let audible = !muted
                    && (!lane.stream
                        || focused.as_ref().and_then(|peer| peer.as_deref()) == Some(&lane.peer));
                if let Ok(mut samples) = lane.queue.try_lock() {
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
        if self.controls.effectively_deafened() {
            stereo.fill(0.0);
        }
        for sample in stereo.iter_mut() {
            *sample = sample.clamp(-0.84139514, 0.84139514);
        }
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
