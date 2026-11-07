use std::future::Future;
use std::sync::{Mutex, MutexGuard};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CriticalAction {
    Leave,
    Logout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntentScope {
    Session,
    Voice,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntentTicket {
    owner: u64,
    voice: u64,
}

#[derive(Default)]
struct State {
    owner: u64,
    voice: u64,
    pending: Option<CriticalAction>,
    changed: CancellationToken,
    owner_stop: CancellationToken,
}

#[derive(Default)]
pub struct CriticalIntents {
    state: Mutex<State>,
}

impl CriticalIntents {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub fn ticket(&self) -> IntentTicket {
        let state = self.state();
        IntentTicket {
            owner: state.owner,
            voice: state.voice,
        }
    }

    pub fn request(&self, action: CriticalAction) {
        let (changed, owner_stop) = {
            let mut state = self.state();
            state.voice = state.voice.saturating_add(1);
            let owner_stop = if action == CriticalAction::Logout {
                state.owner = state.owner.saturating_add(1);
                Some(std::mem::replace(
                    &mut state.owner_stop,
                    CancellationToken::new(),
                ))
            } else {
                None
            };
            if state.pending != Some(CriticalAction::Logout) {
                state.pending = Some(action);
            }
            (
                std::mem::replace(&mut state.changed, CancellationToken::new()),
                owner_stop,
            )
        };
        if let Some(owner_stop) = owner_stop {
            owner_stop.cancel();
        }
        changed.cancel();
    }

    pub fn session_token(&self, ticket: IntentTicket) -> CancellationToken {
        let state = self.state();
        if state.owner == u64::MAX
            || ticket.owner != state.owner
            || state.pending == Some(CriticalAction::Logout)
        {
            let stop = CancellationToken::new();
            stop.cancel();
            stop
        } else {
            state.owner_stop.child_token()
        }
    }

    pub fn voice_context(&self, ticket: IntentTicket) -> IntentTicket {
        IntentTicket {
            owner: ticket.owner,
            voice: self.state().voice,
        }
    }

    pub fn is_current(&self, ticket: IntentTicket, scope: IntentScope) -> bool {
        let state = self.state();
        state.owner != u64::MAX
            && ticket.owner == state.owner
            && state.pending != Some(CriticalAction::Logout)
            && (scope == IntentScope::Session
                || (state.voice != u64::MAX
                    && ticket.voice == state.voice
                    && state.pending.is_none()))
    }

    pub async fn next(&self) -> CriticalAction {
        loop {
            let changed = {
                let mut state = self.state();
                if let Some(action) = state.pending.take() {
                    return action;
                }
                state.changed.clone()
            };
            changed.cancelled().await;
        }
    }

    pub async fn run<T>(
        &self,
        ticket: IntentTicket,
        scope: IntentScope,
        future: impl Future<Output = T>,
    ) -> Option<T> {
        tokio::pin!(future);
        loop {
            let changed = {
                let state = self.state();
                if state.owner == u64::MAX
                    || ticket.owner != state.owner
                    || state.pending == Some(CriticalAction::Logout)
                    || (scope == IntentScope::Voice
                        && (state.voice == u64::MAX
                            || ticket.voice != state.voice
                            || state.pending.is_some()))
                {
                    return None;
                }
                state.changed.clone()
            };
            tokio::select! {
                biased;
                _ = changed.cancelled() => {},
                result = &mut future => return self.is_current(ticket, scope).then_some(result),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn leave_invalidates_voice_but_preserves_session_and_fresh_user_intent() {
        let intents = CriticalIntents::default();
        let old = intents.ticket();
        intents.request(CriticalAction::Leave);
        assert!(!intents.is_current(old, IntentScope::Voice));
        assert!(intents.is_current(old, IntentScope::Session));
        let fresh = intents.ticket();
        assert!(!intents.is_current(fresh, IntentScope::Voice));
        assert_eq!(intents.next().await, CriticalAction::Leave);
        assert!(intents.is_current(fresh, IntentScope::Voice));
        assert!(!intents.is_current(old, IntentScope::Voice));
    }

    #[tokio::test]
    async fn logout_dominates_leave_and_invalidates_every_old_owner_ticket() {
        let intents = CriticalIntents::default();
        let old = intents.ticket();
        intents.request(CriticalAction::Leave);
        intents.request(CriticalAction::Logout);
        intents.request(CriticalAction::Leave);
        assert_eq!(intents.next().await, CriticalAction::Logout);
        assert!(!intents.is_current(old, IntentScope::Session));
        assert!(!intents.is_current(old, IntentScope::Voice));
        assert!(intents.is_current(intents.ticket(), IntentScope::Session));
    }

    #[tokio::test]
    async fn request_before_waiter_is_not_lost_and_old_ready_result_is_rejected() {
        let intents = CriticalIntents::default();
        let old = intents.ticket();
        intents.request(CriticalAction::Leave);
        assert_eq!(
            intents.run(old, IntentScope::Voice, async { true }).await,
            None
        );
        assert_eq!(intents.next().await, CriticalAction::Leave);
        assert_eq!(
            intents.run(old, IntentScope::Voice, async { true }).await,
            None
        );
    }

    #[tokio::test]
    async fn saturated_generations_never_wrap_and_revalidate_old_work() {
        let intents = CriticalIntents::default();
        let old = intents.ticket();
        intents.state().owner = u64::MAX - 1;
        intents.request(CriticalAction::Logout);
        intents.request(CriticalAction::Logout);
        assert_eq!(intents.next().await, CriticalAction::Logout);
        assert!(!intents.is_current(old, IntentScope::Session));
        assert!(!intents.is_current(intents.ticket(), IntentScope::Session));
    }

    #[tokio::test]
    async fn owner_tokens_cancel_on_logout_only_and_children_do_not_revoke_the_owner() {
        let intents = CriticalIntents::default();
        let old = intents.ticket();
        let child = intents.session_token(old);
        child.cancel();
        let live = intents.session_token(old);
        assert!(!live.is_cancelled());
        intents.request(CriticalAction::Leave);
        assert!(!live.is_cancelled());
        assert_eq!(intents.next().await, CriticalAction::Leave);
        intents.request(CriticalAction::Logout);
        assert!(live.is_cancelled());
        assert!(intents.session_token(old).is_cancelled());
        assert!(intents.session_token(intents.ticket()).is_cancelled());
        assert_eq!(intents.next().await, CriticalAction::Logout);
        assert!(intents.session_token(old).is_cancelled());
        assert!(!intents.session_token(intents.ticket()).is_cancelled());
    }
}
