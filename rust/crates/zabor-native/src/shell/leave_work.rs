use super::{HubConnection, MediaEvent, StreamDropGrace, UiEvent, VideoReceiver, VoiceSession};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zabor_native::audio_runtime::AudioDemand;
use zabor_native::call_recovery::CallRecovery;
use zabor_native::critical_intents::{CriticalIntents, IntentScope, IntentTicket};

pub(super) struct VoiceLeave<'a> {
    pub voice: &'a mut Option<VoiceSession>,
    pub media_events: &'a mut Option<mpsc::Receiver<MediaEvent>>,
    pub voice_ticket: &'a mut Option<IntentTicket>,
    pub pending_join: &'a mut Option<String>,
    pub video_jobs: &'a mut BTreeMap<String, VideoReceiver>,
    pub paused_video: &'a mut BTreeSet<String>,
    pub stream_drops: &'a mut StreamDropGrace,
    pub call_recovery: &'a mut CallRecovery,
    pub abandoned_call: &'a mut bool,
    pub pending_stop_stream: &'a mut bool,
    pub call_connected: &'a mut bool,
    pub sent_audio_state: &'a mut Option<(bool, bool)>,
    pub sent_speaking: &'a mut bool,
    pub speaking_release: &'a mut Option<tokio::time::Instant>,
    pub local_speaking: &'a mut bool,
    pub video_generation: &'a AtomicU64,
    pub audio_demand: &'a AudioDemand,
}

impl VoiceLeave<'_> {
    pub(super) async fn leave(
        &mut self,
        intents: &CriticalIntents,
        ticket: IntentTicket,
        stop: &CancellationToken,
        hub: Option<&HubConnection>,
        update: &(dyn Fn(UiEvent) + Send + Sync),
    ) {
        self.audio_demand.reset();
        self.call_recovery.clear();
        *self.pending_join = None;
        self.video_generation.fetch_add(1, Ordering::AcqRel);
        self.video_jobs.clear();
        self.paused_video.clear();
        self.stream_drops.clear();
        *self.abandoned_call |= self
            .voice
            .as_ref()
            .is_some_and(|session| session.call_peer.is_some());
        *self.pending_stop_stream |= self.voice.as_ref().is_some_and(VoiceSession::is_sharing);
        *self.voice = None;
        *self.voice_ticket = None;
        *self.media_events = None;
        *self.call_connected = false;
        *self.sent_audio_state = None;
        *self.sent_speaking = false;
        *self.speaking_release = None;
        *self.local_speaking = false;
        if !stop.is_cancelled() && intents.is_current(ticket, IntentScope::Voice) {
            update(UiEvent::VoiceEnded.guarded(ticket, IntentScope::Voice));
        }
        if let Some(connection) = hub.filter(|connection| connection.is_connected()) {
            let result = send_leave_hints(
                intents,
                ticket,
                stop,
                *self.pending_stop_stream,
                *self.abandoned_call,
                |target| async move { connection.send(target, Vec::new()).await.is_ok() },
            )
            .await;
            if result.stream_stopped {
                *self.pending_stop_stream = false;
            }
            if result.left {
                *self.abandoned_call = false;
            }
        }
    }
}

pub(super) struct LeaveHintResult {
    pub stream_stopped: bool,
    pub left: bool,
}

pub(super) async fn send_leave_hints<F: Future<Output = bool>>(
    intents: &CriticalIntents,
    ticket: IntentTicket,
    stop: &CancellationToken,
    was_sharing: bool,
    was_call: bool,
    mut send: impl FnMut(&'static str) -> F,
) -> LeaveHintResult {
    let mut result = LeaveHintResult {
        stream_stopped: !was_sharing,
        left: false,
    };
    if was_sharing {
        result.stream_stopped = tokio::select! {
            biased;
            _ = stop.cancelled() => false,
            sent = intents.run(ticket, IntentScope::Voice, tokio::time::timeout(Duration::from_millis(250), send("StopStream"))) => matches!(sent, Some(Ok(true))),
        };
    }
    result.left = tokio::select! {
        biased;
        _ = stop.cancelled() => false,
        sent = intents.run(ticket, IntentScope::Voice, tokio::time::timeout(Duration::from_millis(750), send(if was_call { "EndCall" } else { "LeaveChannel" }))) => matches!(sent, Some(Ok(true))),
    };
    result
}
