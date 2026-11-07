use super::UiEvent;
use super::{leave_work::VoiceLeave, HubConnection};
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

    pub(super) fn commit<T>(&self, commit: impl FnOnce() -> T) -> Option<T> {
        self.intents
            .commit(self.ticket, IntentScope::Session, || {
                (!self.stop.is_cancelled()).then(commit)
            })
            .flatten()
    }

    pub(super) async fn run<T>(&self, future: impl Future<Output = T>) -> Option<T> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => None,
            result = self.intents.run(self.ticket, IntentScope::Session, future) => result.filter(|_| self.is_current()),
        }
    }

    pub(super) async fn run_with_leave<T>(
        &self,
        future: impl Future<Output = T>,
        leave: &mut VoiceLeave<'_>,
        hub: Option<&HubConnection>,
    ) -> Option<T> {
        let future = self.run(future);
        tokio::pin!(future);
        loop {
            tokio::select! {
                biased;
                _ = self.stop.cancelled() => return None,
                ticket = self.intents.next_leave(self.ticket) => {
                    let ticket = ticket?;
                    leave.leave(self.intents, ticket, self.stop, hub, self.update).await;
                },
                result = &mut future => {
                    if let Some(ticket) = self.intents.take_leave(self.ticket) {
                        leave.leave(self.intents, ticket, self.stop, hub, self.update).await;
                    }
                    return result.filter(|_| self.is_current());
                },
            }
        }
    }
}
