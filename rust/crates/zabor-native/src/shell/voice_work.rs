use super::{HubConnection, MediaEvent, UiEvent, VoiceSession};
use serde_json::Value;
use std::future::Future;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zabor_native::critical_intents::{CriticalIntents, IntentScope, IntentTicket};
use zabor_native::hub::HubError;
use zabor_native::media::PeerLifetime;

type VoiceResources = (Option<VoiceSession>, Option<mpsc::Receiver<MediaEvent>>);

pub(super) struct VoiceWork<'a> {
    intents: &'a CriticalIntents,
    ticket: IntentTicket,
    stop: &'a CancellationToken,
    update: &'a (dyn Fn(UiEvent) + Send + Sync),
}

impl<'a> VoiceWork<'a> {
    pub(super) fn new(
        intents: &'a CriticalIntents,
        ticket: IntentTicket,
        stop: &'a CancellationToken,
        update: &'a (dyn Fn(UiEvent) + Send + Sync),
    ) -> Self {
        Self {
            intents,
            ticket,
            stop,
            update,
        }
    }

    pub(super) fn is_current(&self) -> bool {
        !self.stop.is_cancelled() && self.intents.is_current(self.ticket, IntentScope::Voice)
    }

    pub(super) fn commit<T>(&self, commit: impl FnOnce() -> T) -> Option<T> {
        self.intents
            .commit(self.ticket, IntentScope::Voice, || {
                (!self.stop.is_cancelled()).then(commit)
            })
            .flatten()
    }

    pub(super) fn commit_resource<T>(
        &self,
        resource_ticket: Option<IntentTicket>,
        mut accepted: impl FnMut() -> T,
    ) -> Option<T> {
        self.commit(|| (resource_ticket == Some(self.ticket)).then(&mut accepted))
            .flatten()
    }

    pub(super) fn take_voice(
        &self,
        voice: &mut Option<VoiceSession>,
        events: &mut Option<mpsc::Receiver<MediaEvent>>,
        resource_ticket: Option<IntentTicket>,
        mut accepted: impl FnMut(Option<&VoiceSession>),
    ) -> Option<VoiceResources> {
        let mut resources = None;
        let taken = self.commit(|| {
            if resource_ticket != Some(self.ticket) {
                return false;
            }
            resources = Some((voice.take(), events.take()));
            accepted(
                resources
                    .as_ref()
                    .and_then(|resources| resources.0.as_ref()),
            );
            true
        });
        taken.filter(|taken| *taken).and(resources)
    }

    pub(super) fn take_previous_voice(
        &self,
        voice: &mut Option<VoiceSession>,
        events: &mut Option<mpsc::Receiver<MediaEvent>>,
        resource_ticket: Option<IntentTicket>,
        retain_events: bool,
        mut accepted: impl FnMut(Option<&VoiceSession>),
    ) -> Option<VoiceResources> {
        let mut resources = None;
        let taken = self.commit(|| {
            if (voice.is_some() || events.is_some()) && resource_ticket != Some(self.ticket) {
                return false;
            }
            resources = Some((
                voice.take(),
                if retain_events { None } else { events.take() },
            ));
            accepted(
                resources
                    .as_ref()
                    .and_then(|resources| resources.0.as_ref()),
            );
            true
        });
        taken.filter(|taken| *taken).and(resources)
    }

    pub(super) fn install_voice(
        &self,
        voice: &mut Option<VoiceSession>,
        events: &mut Option<mpsc::Receiver<MediaEvent>>,
        resource_ticket: &mut Option<IntentTicket>,
        prepared: (VoiceSession, mpsc::Receiver<MediaEvent>),
        mut accepted: impl FnMut(),
    ) -> Option<VoiceResources> {
        let mut prepared = Some(prepared);
        let mut replaced = None;
        let installed = self.commit(|| {
            let (session, receiver) = prepared.take().unwrap();
            replaced = Some((voice.replace(session), events.replace(receiver)));
            *resource_ticket = Some(self.ticket);
            accepted();
            true
        });
        installed.filter(|installed| *installed).and(replaced)
    }

    pub(super) fn publish(&self, event: UiEvent) {
        if self.is_current() {
            (self.update)(event.guarded(self.ticket, IntentScope::Voice));
        }
    }

    pub(super) fn publish_peer(&self, lifetime: &PeerLifetime, event: UiEvent) {
        if lifetime.is_alive() {
            self.publish(UiEvent::PeerGuarded(lifetime.clone(), Box::new(event)));
        }
    }

    pub(super) async fn run<T>(&self, future: impl Future<Output = T>) -> Option<T> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => None,
            result = self.intents.run(self.ticket, IntentScope::Voice, future) => result.filter(|_| self.is_current()),
        }
    }

    pub(super) async fn run_peer<T>(
        &self,
        lifetime: &PeerLifetime,
        future: impl Future<Output = T>,
    ) -> Option<T> {
        if !lifetime.is_alive() {
            return None;
        }
        tokio::select! {
            biased;
            _ = lifetime.cancelled() => None,
            result = self.run(future) => result.filter(|_| lifetime.is_alive() && self.is_current()),
        }
    }

    pub(super) async fn send_peer(
        &self,
        lifetime: &PeerLifetime,
        hub: &HubConnection,
        target: &str,
        arguments: Vec<Value>,
    ) -> Option<Result<(), HubError>> {
        self.run_peer(lifetime, hub.send(target, arguments)).await
    }
}
