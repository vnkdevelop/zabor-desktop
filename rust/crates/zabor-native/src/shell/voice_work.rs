use super::{HubConnection, UiEvent};
use serde_json::Value;
use std::future::Future;
use tokio_util::sync::CancellationToken;
use zabor_native::critical_intents::{CriticalIntents, IntentScope, IntentTicket};
use zabor_native::hub::HubError;
use zabor_native::media::PeerLifetime;

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
