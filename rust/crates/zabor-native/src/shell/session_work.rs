use super::UiEvent;
use std::future::Future;
use tokio_util::sync::CancellationToken;
use zabor_native::critical_intents::{CriticalIntents, IntentScope, IntentTicket};

pub(super) struct SessionWork<'a> {
    intents: &'a CriticalIntents,
    ticket: IntentTicket,
    stop: &'a CancellationToken,
    update: &'a (dyn Fn(UiEvent) + Send + Sync),
}

impl<'a> SessionWork<'a> {
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
        !self.stop.is_cancelled() && self.intents.is_current(self.ticket, IntentScope::Session)
    }

    pub(super) fn publish(&self, event: UiEvent) {
        if self.is_current() {
            (self.update)(event.guarded(self.ticket, IntentScope::Session));
        }
    }

    pub(super) async fn run<T>(&self, future: impl Future<Output = T>) -> Option<T> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => None,
            result = self.intents.run(self.ticket, IntentScope::Session, future) => result.filter(|_| self.is_current()),
        }
    }
}
