use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

pub const SAMPLE_RATE: usize = 48000;
pub const CLIP_SAMPLES: usize = 5 * SAMPLE_RATE;
pub const MONITOR_SETTLE: Duration = Duration::from_millis(120);
pub const PIPELINE_FLUSH: Duration = Duration::from_millis(280);
pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(9);
pub const PREPARATION_TIMEOUT: Duration = Duration::from_secs(30);
pub const IDLE_RELEASE: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MicTestStage {
    #[default]
    Idle,
    Arming,
    Recording,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicTestIssue {
    Silent,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicTestFailure {
    PreparationTimeout,
    CaptureTimeout,
    Processor,
    InvalidFrame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureToken(u64, u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackTicket {
    generation: u64,
    revision: u64,
    sequence: u64,
    route: u64,
    offset: usize,
    samples: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MicTestSnapshot {
    pub stage: MicTestStage,
    pub issue: Option<MicTestIssue>,
    pub failure: Option<MicTestFailure>,
    pub visible: bool,
    pub playing: bool,
    pub captured_samples: usize,
    pub stored_samples: usize,
    pub duration_seconds: f64,
    pub position_seconds: f64,
    pub peak_db: Option<i32>,
    pub meter_db: f32,
}

impl Default for MicTestSnapshot {
    fn default() -> Self {
        Self {
            stage: MicTestStage::Idle,
            issue: None,
            failure: None,
            visible: false,
            playing: false,
            captured_samples: 0,
            stored_samples: 0,
            duration_seconds: 0.0,
            position_seconds: 0.0,
            peak_db: None,
            meter_db: -100.0,
        }
    }
}

struct Capture {
    preparation_deadline: Instant,
    attached: Option<Instant>,
    recording: Option<Instant>,
    buffer: Vec<f32>,
    written: usize,
    peak: f32,
}

#[derive(Default)]
struct State {
    generation: u64,
    input_revision: u64,
    enabled: bool,
    visible: bool,
    stage: MicTestStage,
    issue: Option<MicTestIssue>,
    failure: Option<MicTestFailure>,
    capture: Option<Capture>,
    clip: Option<Vec<f32>>,
    peak_db: Option<i32>,
    meter_db: f32,
    position: usize,
    playing: bool,
    playback_revision: u64,
    render_sequence: u64,
    route: Option<u64>,
    pending: Option<PlaybackTicket>,
}

impl State {
    fn revoke_playback(&mut self) {
        self.playback_revision = self.playback_revision.wrapping_add(1);
        self.pending = None;
    }

    fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.revoke_playback();
        self.capture = None;
        self.clip = None;
        self.peak_db = None;
        self.meter_db = -100.0;
        self.position = 0;
        self.playing = false;
        self.stage = MicTestStage::Idle;
        self.issue = None;
        self.failure = None;
        self.route = None;
    }

    fn fail(&mut self, reason: MicTestFailure) {
        self.clear();
        self.issue = Some(MicTestIssue::Failed);
        self.failure = Some(reason);
    }

    fn advance(&mut self, now: Instant) {
        let Some(capture) = &mut self.capture else {
            return;
        };
        let failure = if let Some(attached) = capture.attached {
            (now.saturating_duration_since(attached) >= CAPTURE_TIMEOUT)
                .then_some(MicTestFailure::CaptureTimeout)
        } else {
            (now >= capture.preparation_deadline).then_some(MicTestFailure::PreparationTimeout)
        };
        if let Some(failure) = failure {
            self.fail(failure);
            return;
        }
        if capture.recording.is_none()
            && capture
                .attached
                .is_some_and(|attached| now.saturating_duration_since(attached) >= MONITOR_SETTLE)
        {
            capture.recording = Some(now);
            self.stage = MicTestStage::Recording;
        }
    }

    fn valid_playback(&self, ticket: PlaybackTicket, route: u64) -> bool {
        self.enabled
            && self.visible
            && self.playing
            && self.generation == ticket.generation
            && self.playback_revision == ticket.revision
            && ticket.route == route
            && self.pending == Some(ticket)
            && self.position == ticket.offset
    }
}

pub struct MicTest {
    state: Mutex<State>,
}

pub(crate) struct MicTestDispose<'a> {
    state: MutexGuard<'a, State>,
    replaced: State,
    applied: bool,
}

pub(crate) struct MicTestMaintenance<'a> {
    state: MutexGuard<'a, State>,
    replaced: State,
    now: Instant,
    failure: Option<MicTestFailure>,
    applied: bool,
}

impl MicTestMaintenance<'_> {
    pub(crate) fn has_demand(&self) -> bool {
        self.state.enabled
            && (self.state.visible
                || self.failure.is_none() && (self.state.capture.is_some() || self.state.playing))
    }

    pub(crate) fn apply(&mut self, processor_failed: bool) {
        if self.applied {
            return;
        }
        if let Some(failure) = self.failure {
            self.replaced.issue = Some(MicTestIssue::Failed);
            self.replaced.failure = Some(failure);
            std::mem::swap(&mut *self.state, &mut self.replaced);
        } else {
            self.state.advance(self.now);
        }
        if processor_failed
            && self.state.enabled
            && (self.state.visible
                || self
                    .state
                    .capture
                    .as_ref()
                    .is_some_and(|capture| capture.attached.is_some()))
        {
            if self.state.capture.is_some() {
                self.replaced.issue = Some(MicTestIssue::Failed);
                self.replaced.failure = Some(MicTestFailure::Processor);
                std::mem::swap(&mut *self.state, &mut self.replaced);
            } else {
                if self.state.failure != Some(MicTestFailure::Processor) {
                    self.state.input_revision = self.state.input_revision.wrapping_add(1);
                }
                self.state.meter_db = -100.0;
                self.state.issue = Some(MicTestIssue::Failed);
                self.state.failure = Some(MicTestFailure::Processor);
            }
        }
        self.applied = true;
    }
}

impl MicTestDispose<'_> {
    pub(crate) fn apply(&mut self) {
        if !self.applied {
            std::mem::swap(&mut *self.state, &mut self.replaced);
            self.applied = true;
        }
    }
}

impl Default for MicTest {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                meter_db: -100.0,
                ..State::default()
            }),
        }
    }
}

impl MicTest {
    pub fn configure(&self, enabled: bool, visible: bool) {
        if let Ok(mut state) = self.state.lock() {
            if !enabled && state.enabled {
                state.clear();
            }
            state.enabled = enabled;
            state.visible = enabled && visible;
            if !state.visible {
                state.playing = false;
                state.meter_db = -100.0;
                state.revoke_playback();
            }
        }
    }

    pub fn dispose(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.clear();
            state.enabled = false;
            state.visible = false;
        }
    }

    pub(crate) fn try_prepare_dispose(&self) -> Result<Option<MicTestDispose<'_>>, &'static str> {
        let state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => return Err("microphone test unavailable"),
        };
        let replaced = State {
            generation: state.generation.wrapping_add(1),
            input_revision: state.input_revision,
            playback_revision: state.playback_revision.wrapping_add(1),
            render_sequence: state.render_sequence,
            meter_db: -100.0,
            ..State::default()
        };
        Ok(Some(MicTestDispose {
            state,
            replaced,
            applied: false,
        }))
    }

    pub(crate) fn prepare_maintenance(&self, now: Instant) -> Option<MicTestMaintenance<'_>> {
        let state = self.state.lock().ok()?;
        let failure = state.capture.as_ref().and_then(|capture| {
            if let Some(attached) = capture.attached {
                (now.saturating_duration_since(attached) >= CAPTURE_TIMEOUT)
                    .then_some(MicTestFailure::CaptureTimeout)
            } else {
                (now >= capture.preparation_deadline).then_some(MicTestFailure::PreparationTimeout)
            }
        });
        let replaced = State {
            generation: state.generation.wrapping_add(1),
            input_revision: state.input_revision,
            enabled: state.enabled,
            visible: state.visible,
            playback_revision: state.playback_revision.wrapping_add(1),
            render_sequence: state.render_sequence,
            meter_db: -100.0,
            ..State::default()
        };
        Some(MicTestMaintenance {
            state,
            replaced,
            now,
            failure,
            applied: false,
        })
    }

    pub fn begin(&self, now: Instant) -> Option<CaptureToken> {
        let mut state = self.state.lock().ok()?;
        state.advance(now);
        if !state.enabled || !state.visible {
            return None;
        }
        if state.capture.is_none() {
            state.clear();
            state.stage = MicTestStage::Arming;
            state.capture = Some(Capture {
                preparation_deadline: now + PREPARATION_TIMEOUT,
                attached: None,
                recording: None,
                buffer: vec![0.0; CLIP_SAMPLES],
                written: 0,
                peak: 0.0,
            });
        }
        Some(CaptureToken(state.generation, state.input_revision))
    }

    pub fn prepared(&self, token: CaptureToken, now: Instant) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        state.advance(now);
        if token.0 != state.generation || token.1 != state.input_revision || !state.enabled {
            return false;
        }
        let Some(capture) = &mut state.capture else {
            return false;
        };
        if capture.attached.is_none() {
            capture.attached = Some(now);
        }
        true
    }

    pub fn capture_token(&self) -> Option<CaptureToken> {
        let state = self.state.lock().ok()?;
        (state.enabled
            && (state.visible
                || state
                    .capture
                    .as_ref()
                    .is_some_and(|run| run.attached.is_some())))
        .then_some(CaptureToken(state.generation, state.input_revision))
    }

    pub fn capture(&self, token: CaptureToken, pcm: &[f32; 480], db: f32, now: Instant) -> bool {
        let Ok(mut state) = self.state.try_lock() else {
            return false;
        };
        state.advance(now);
        if token.0 != state.generation || token.1 != state.input_revision || !state.enabled {
            return false;
        }
        if pcm.iter().any(|sample| !sample.is_finite()) || !db.is_finite() {
            state.fail(MicTestFailure::InvalidFrame);
            return false;
        }
        if state.visible {
            state.meter_db = db.clamp(-100.0, 0.0);
        }
        if state.capture.is_none() && state.failure == Some(MicTestFailure::Processor) {
            state.failure = None;
            state.issue = state
                .peak_db
                .filter(|db| *db <= -70)
                .map(|_| MicTestIssue::Silent);
        }
        let Some(capture) = &mut state.capture else {
            return true;
        };
        if !capture
            .recording
            .is_some_and(|recording| now.saturating_duration_since(recording) >= PIPELINE_FLUSH)
        {
            return true;
        }
        capture.buffer[capture.written..capture.written + 480].copy_from_slice(pcm);
        capture.written += 480;
        for sample in pcm {
            capture.peak = capture.peak.max(sample.abs());
        }
        if capture.written == CLIP_SAMPLES {
            let Some(capture) = state.capture.take() else {
                return false;
            };
            let db = (20.0 * f64::from(capture.peak).max(1e-6).log10() + 0.5).floor() as i32;
            state.clip = Some(capture.buffer);
            state.peak_db = Some(db);
            state.issue = (db <= -70).then_some(MicTestIssue::Silent);
            state.stage = MicTestStage::Ready;
            state.position = 0;
            state.playing = state.visible;
            state.revoke_playback();
        }
        true
    }

    pub fn fail(&self, token: CaptureToken, reason: MicTestFailure) {
        if let Ok(mut state) = self.state.lock() {
            if state.enabled && state.generation == token.0 && state.input_revision == token.1 {
                if state.capture.is_some() {
                    state.fail(reason);
                } else if reason == MicTestFailure::Processor {
                    if state.failure != Some(reason) {
                        state.input_revision = state.input_revision.wrapping_add(1);
                    }
                    state.meter_db = -100.0;
                    state.issue = Some(MicTestIssue::Failed);
                    state.failure = Some(reason);
                }
            }
        }
    }

    pub fn snapshot(&self, now: Instant) -> MicTestSnapshot {
        let Ok(mut state) = self.state.lock() else {
            return MicTestSnapshot {
                issue: Some(MicTestIssue::Failed),
                meter_db: -100.0,
                ..MicTestSnapshot::default()
            };
        };
        state.advance(now);
        let length = state.clip.as_ref().map_or(0, Vec::len);
        MicTestSnapshot {
            stage: state.stage,
            issue: state.issue,
            failure: state.failure,
            visible: state.visible,
            playing: state.playing,
            captured_samples: state
                .capture
                .as_ref()
                .map_or(length, |capture| capture.written),
            stored_samples: state
                .capture
                .as_ref()
                .map_or(length, |capture| capture.buffer.len()),
            duration_seconds: length as f64 / SAMPLE_RATE as f64,
            position_seconds: state.position as f64 / SAMPLE_RATE as f64,
            peak_db: state.peak_db,
            meter_db: state.meter_db,
        }
    }

    pub fn has_demand(&self) -> bool {
        self.state.lock().is_ok_and(|state| {
            state.enabled && (state.visible || state.capture.is_some() || state.playing)
        })
    }

    pub fn play(&self, offset_seconds: Option<f64>) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(clip) = &state.clip else {
            return false;
        };
        if !state.enabled || !state.visible {
            return false;
        }
        let length = clip.len();
        let offset = offset_seconds.unwrap_or(state.position as f64 / SAMPLE_RATE as f64);
        if !offset.is_finite() {
            return false;
        }
        state.position = if offset >= (length as f64 / SAMPLE_RATE as f64 - 0.02) {
            0
        } else {
            (offset.max(0.0) * SAMPLE_RATE as f64).floor() as usize
        };
        state.playing = true;
        state.revoke_playback();
        true
    }

    pub fn pause(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.playing = false;
            state.revoke_playback();
        }
    }

    pub fn seek(&self, seconds: f64) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(clip) = &state.clip else {
            return false;
        };
        if !seconds.is_finite() || !state.enabled {
            return false;
        }
        let limit = clip.len() as f64 / SAMPLE_RATE as f64 - 0.05;
        state.position = (seconds.clamp(0.0, limit) * SAMPLE_RATE as f64).floor() as usize;
        state.revoke_playback();
        true
    }

    pub fn render(&self, stereo: &mut [f32], route: u64, gain: f32) -> Option<PlaybackTicket> {
        stereo.fill(0.0);
        if stereo.is_empty()
            || !stereo.len().is_multiple_of(2)
            || stereo.len() > SAMPLE_RATE * 2
            || !gain.is_finite()
        {
            return None;
        }
        let mut state = self.state.try_lock().ok()?;
        if !state.enabled || !state.visible || !state.playing {
            return None;
        }
        if state.route != Some(route) {
            state.route = Some(route);
            state.revoke_playback();
        }
        let clip = state.clip.as_ref()?;
        let count = (stereo.len() / 2).min(clip.len() - state.position);
        let gain = gain.clamp(0.0, 2.0);
        for (frame, sample) in stereo
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(&clip[state.position..state.position + count])
        {
            frame.fill((*sample * gain).clamp(-1.0, 1.0));
        }
        state.render_sequence = state.render_sequence.wrapping_add(1);
        let ticket = PlaybackTicket {
            generation: state.generation,
            revision: state.playback_revision,
            sequence: state.render_sequence,
            route,
            offset: state.position,
            samples: count,
        };
        state.pending = Some(ticket);
        Some(ticket)
    }

    pub fn validate_playback(&self, ticket: PlaybackTicket, route: u64) -> bool {
        self.state
            .try_lock()
            .is_ok_and(|state| state.valid_playback(ticket, route))
    }

    pub fn played(&self, ticket: PlaybackTicket, route: u64) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if !state.valid_playback(ticket, route) {
            return false;
        }
        state.pending = None;
        state.position += ticket.samples;
        if state
            .clip
            .as_ref()
            .is_some_and(|clip| state.position == clip.len())
        {
            state.playing = false;
            state.revoke_playback();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_snapshot_never_reports_a_live_or_loud_microphone() {
        let snapshot = MicTestSnapshot::default();
        assert_eq!(snapshot.meter_db, -100.0);
        assert_eq!(snapshot.stage, MicTestStage::Idle);
        assert!(!snapshot.playing);
        assert_eq!(snapshot.stored_samples, 0);
    }

    #[test]
    fn successful_write_ack_waits_for_the_state_lock_instead_of_losing_playback_progress() {
        let test = MicTest::default();
        let ticket = PlaybackTicket {
            generation: 0,
            revision: 0,
            sequence: 1,
            route: 7,
            offset: 0,
            samples: 480,
        };
        let mut state = test.state.lock().unwrap();
        state.enabled = true;
        state.visible = true;
        state.playing = true;
        state.clip = Some(vec![0.1; CLIP_SAMPLES]);
        state.pending = Some(ticket);
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (complete_tx, complete_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                entered_tx.send(()).unwrap();
                complete_tx.send(test.played(ticket, 7)).unwrap();
            });
            entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(
                complete_rx.recv_timeout(Duration::from_millis(30)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            );
            drop(state);
            assert!(complete_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        });
        assert_eq!(test.snapshot(Instant::now()).position_seconds, 0.01);
        assert!(!test.played(ticket, 7));
    }

    #[test]
    fn prepared_dispose_preserves_discarded_clip_and_revokes_once_without_dropping_it_during_apply()
    {
        let test = MicTest::default();
        let now = Instant::now();
        test.configure(true, true);
        let token = test.begin(now).unwrap();
        test.prepared(token, now);
        test.snapshot(now + MONITOR_SETTLE);
        for index in 0..500 {
            assert!(test.capture(
                token,
                &[0.1; 480],
                -20.0,
                now + MONITOR_SETTLE + PIPELINE_FLUSH + Duration::from_millis(index * 10),
            ));
        }
        let original = test.snapshot(now);
        let ticket = test.render(&mut [0.0; 960], 7, 1.0).unwrap();
        drop(test.try_prepare_dispose().unwrap().unwrap());
        assert_eq!(test.snapshot(now), original);
        assert!(test.validate_playback(ticket, 7));
        let mut prepared = test.try_prepare_dispose().unwrap().unwrap();
        assert!(test.try_prepare_dispose().unwrap().is_none());
        prepared.apply();
        let generation = prepared.state.generation;
        prepared.apply();
        assert_eq!(prepared.state.generation, generation);
        assert_eq!(prepared.replaced.clip.as_ref().unwrap().len(), CLIP_SAMPLES);
        assert!(prepared.state.clip.is_none());
        drop(prepared);
        assert!(!test.has_demand());
        assert!(!test.validate_playback(ticket, 7));
        assert!(!test.capture(token, &[0.1; 480], -20.0, now));
        assert_eq!(test.snapshot(now), MicTestSnapshot::default());
    }

    #[test]
    fn prepared_maintenance_matches_legacy_timing_and_failure_with_deferred_buffer_destruction() {
        for mode in [
            "arming",
            "recording",
            "preparation-timeout",
            "capture-timeout",
            "hidden",
            "ready",
            "disabled",
        ] {
            for processor_failed in [false, true] {
                let legacy = MicTest::default();
                let guarded = MicTest::default();
                let now = Instant::now();
                for test in [&legacy, &guarded] {
                    test.configure(true, true);
                    let token = test.begin(now).unwrap();
                    if ["recording", "capture-timeout", "hidden", "ready"].contains(&mode) {
                        assert!(test.prepared(token, now));
                    }
                    if mode == "ready" {
                        test.snapshot(now + MONITOR_SETTLE);
                        for index in 0..500 {
                            assert!(test.capture(
                                token,
                                &[0.1; 480],
                                -20.0,
                                now + MONITOR_SETTLE
                                    + PIPELINE_FLUSH
                                    + Duration::from_millis(index * 10)
                            ));
                        }
                    } else if mode == "hidden" {
                        test.configure(true, false);
                    } else if mode == "disabled" {
                        test.configure(false, false);
                    }
                }
                let time = match mode {
                    "preparation-timeout" => now + PREPARATION_TIMEOUT,
                    "capture-timeout" => now + CAPTURE_TIMEOUT,
                    _ => now + MONITOR_SETTLE,
                };
                let original = guarded.snapshot(now);
                let token = guarded.capture_token();
                drop(guarded.prepare_maintenance(time).unwrap());
                assert_eq!(guarded.snapshot(now), original);
                assert_eq!(guarded.capture_token(), token);
                let mut plan = guarded.prepare_maintenance(time).unwrap();
                let buffer = plan
                    .state
                    .capture
                    .as_ref()
                    .map(|capture| capture.buffer.as_ptr());
                legacy.snapshot(time);
                assert_eq!(plan.has_demand(), legacy.has_demand());
                if processor_failed {
                    if let Some(token) = legacy.capture_token() {
                        legacy.fail(token, MicTestFailure::Processor);
                    }
                }
                plan.apply(processor_failed);
                let generation = plan.state.generation;
                plan.apply(processor_failed);
                assert_eq!(plan.state.generation, generation);
                if let Some(buffer) = buffer {
                    if plan.state.capture.is_none() {
                        let retired = plan.replaced.capture.as_ref().unwrap();
                        assert_eq!(retired.buffer.as_ptr(), buffer);
                        assert_eq!(retired.buffer.len(), CLIP_SAMPLES);
                    }
                }
                let state = legacy.state.lock().unwrap();
                assert_eq!(plan.state.generation, state.generation);
                assert_eq!(plan.state.input_revision, state.input_revision);
                assert_eq!(plan.state.playback_revision, state.playback_revision);
                assert_eq!(plan.state.render_sequence, state.render_sequence);
                assert_eq!(plan.state.pending, state.pending);
                assert_eq!(plan.state.route, state.route);
                drop(state);
                drop(plan);
                assert_eq!(guarded.snapshot(time), legacy.snapshot(time));
                assert_eq!(guarded.capture_token(), legacy.capture_token());
                println!("mic maintenance mode={mode} processor_failed={processor_failed} abandoned_unchanged=true apply_once=true legacy_result=true buffer_destruction_deferred=true physical_audio=false");
            }
        }
    }
}
