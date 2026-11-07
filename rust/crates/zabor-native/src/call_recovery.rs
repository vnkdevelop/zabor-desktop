use crate::auth::User;
use crate::hub::{HubConnection, HubError};
use serde::Deserialize;
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct CallRecovery {
    peer: Option<String>,
    session: Option<String>,
    connected: bool,
    deadline: Option<Instant>,
    retry_at: Option<Instant>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RecoveryAction {
    Wait,
    End,
    Resume {
        session: String,
        peer: String,
        timeout: Duration,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ResumeResult {
    Resumed { peer: Box<User> },
    Pending,
    Ended,
}

pub fn valid_session(session: &str) -> bool {
    session.len() == 32
        && session
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub async fn resume(
    hub: &HubConnection,
    session: &str,
    duration: Duration,
) -> Result<ResumeResult, HubError> {
    if !valid_session(session) {
        return Err(HubError::Protocol);
    }
    if duration.is_zero() {
        return Err(HubError::Timeout);
    }
    let value = hub
        .invoke(
            "ResumeCall",
            vec![json!(session)],
            duration.min(Duration::from_secs(5)),
        )
        .await?;
    serde_json::from_value(value).map_err(|_| HubError::Protocol)
}

impl CallRecovery {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn begin(&mut self, peer: &str) {
        self.clear();
        if !peer.is_empty() {
            self.peer = Some(peer.into());
        }
    }

    pub fn connected(&mut self, peer: &str) -> bool {
        if self.peer.as_deref() != Some(peer) {
            return false;
        }
        self.connected = true;
        true
    }

    pub fn set_session(&mut self, session: &str) -> bool {
        if !self.connected
            || !valid_session(session)
            || self
                .session
                .as_deref()
                .is_some_and(|current| current != session)
        {
            return false;
        }
        self.session = Some(session.into());
        true
    }

    pub fn suspended(&mut self, session: &str, remaining_ms: u64, now: Instant) -> bool {
        if !self.set_session(session) {
            return false;
        }
        self.deadline
            .get_or_insert(now + Duration::from_millis(remaining_ms.min(45_000)));
        self.retry(now);
        true
    }

    pub fn disconnected(&mut self, now: Instant) {
        if self.connected && self.session.is_some() {
            self.deadline.get_or_insert(now + Duration::from_secs(45));
            self.retry(now);
        }
    }

    pub fn session_for_login(&self, now: Instant) -> Option<&str> {
        self.deadline
            .filter(|deadline| now < *deadline)
            .and(self.session.as_deref())
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.deadline
            .map(|deadline| self.retry_at.unwrap_or(deadline).min(deadline))
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(now))
            .unwrap_or_default()
    }

    pub fn poll(&mut self, now: Instant, session_ready: bool) -> RecoveryAction {
        let Some(deadline) = self.deadline else {
            return RecoveryAction::Wait;
        };
        if now >= deadline {
            return RecoveryAction::End;
        }
        if self.retry_at.is_none_or(|retry| now < retry) {
            return RecoveryAction::Wait;
        }
        self.retry_at = None;
        if !session_ready {
            self.retry(now);
            return RecoveryAction::Wait;
        }
        let (Some(session), Some(peer)) = (&self.session, &self.peer) else {
            return RecoveryAction::End;
        };
        RecoveryAction::Resume {
            session: session.clone(),
            peer: peer.clone(),
            timeout: deadline
                .saturating_duration_since(now)
                .min(Duration::from_secs(5)),
        }
    }

    pub fn retry(&mut self, now: Instant) {
        if let Some(deadline) = self.deadline {
            self.retry_at
                .get_or_insert((now + Duration::from_secs(1)).min(deadline));
        }
    }

    pub fn can_restore(&self, session: &str, peer: &str, now: Instant) -> bool {
        self.connected
            && self.session.as_deref() == Some(session)
            && self.peer.as_deref() == Some(peer)
            && self.deadline.is_some_and(|deadline| now < deadline)
    }

    pub fn restored(&mut self, session: &str, peer: &str, now: Instant) -> bool {
        if !self.can_restore(session, peer, now) {
            return false;
        }
        self.deadline = None;
        self.retry_at = None;
        true
    }

    pub fn accepts_end(&self, session: Option<&str>) -> bool {
        self.session.as_deref() == session
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "0123456789abcdef0123456789abcdef";
    const OTHER: &str = "abcdef0123456789abcdef0123456789";

    fn active() -> CallRecovery {
        let mut recovery = CallRecovery::default();
        recovery.begin("peer");
        assert!(recovery.connected("peer"));
        assert!(recovery.set_session(SESSION));
        recovery
    }

    #[test]
    fn sessions_require_connected_call_and_lowercase_32_digit_hex() {
        let mut recovery = CallRecovery::default();
        assert!(!recovery.set_session(SESSION));
        recovery.begin("peer");
        assert!(!recovery.connected("other"));
        assert!(recovery.connected("peer"));
        for session in [
            "",
            "abc",
            "0123456789ABCDEF0123456789ABCDEF",
            "z123456789abcdef0123456789abcdef",
        ] {
            assert!(!recovery.set_session(session));
        }
        assert!(recovery.set_session(SESSION));
        assert!(!recovery.set_session(OTHER));
    }

    #[test]
    fn duplicate_suspension_and_disconnect_never_extend_deadline() {
        let now = Instant::now();
        let mut recovery = active();
        assert!(recovery.suspended(SESSION, u64::MAX, now));
        assert!(recovery.suspended(SESSION, 45_000, now + Duration::from_secs(20)));
        recovery.disconnected(now + Duration::from_secs(30));
        assert_eq!(
            recovery.poll(now + Duration::from_secs(45), true),
            RecoveryAction::End
        );
        assert_eq!(
            recovery.session_for_login(now + Duration::from_secs(45)),
            None
        );
    }

    #[test]
    fn offline_retries_wait_for_auth_without_opening_another_connection() {
        let now = Instant::now();
        let mut recovery = active();
        recovery.disconnected(now);
        assert_eq!(recovery.session_for_login(now), Some(SESSION));
        assert_eq!(
            recovery.poll(now + Duration::from_secs(1), false),
            RecoveryAction::Wait
        );
        assert_eq!(recovery.next_deadline(), Some(now + Duration::from_secs(2)));
        assert!(matches!(
            recovery.poll(now + Duration::from_secs(2), true),
            RecoveryAction::Resume { .. }
        ));
        assert_eq!(
            recovery.poll(now + Duration::from_secs(2), true),
            RecoveryAction::Wait
        );
    }

    #[test]
    fn requests_are_single_flight_and_bounded_by_remaining_window() {
        let now = Instant::now();
        let mut recovery = active();
        assert!(recovery.suspended(SESSION, 1500, now));
        assert_eq!(
            recovery.poll(now + Duration::from_secs(1), true),
            RecoveryAction::Resume {
                session: SESSION.into(),
                peer: "peer".into(),
                timeout: Duration::from_millis(500),
            }
        );
        assert_eq!(
            recovery.poll(now + Duration::from_secs(1), true),
            RecoveryAction::Wait
        );
        recovery.retry(now + Duration::from_millis(1400));
        assert_eq!(
            recovery.next_deadline(),
            Some(now + Duration::from_millis(1500))
        );
        assert_eq!(
            recovery.poll(now + Duration::from_millis(1500), true),
            RecoveryAction::End
        );
    }

    #[test]
    fn stale_end_events_cannot_end_new_session_and_legacy_end_still_works() {
        let recovery = active();
        assert!(!recovery.accepts_end(None));
        assert!(!recovery.accepts_end(Some(OTHER)));
        assert!(recovery.accepts_end(Some(SESSION)));
        let mut legacy = CallRecovery::default();
        legacy.begin("peer");
        assert!(legacy.accepts_end(None));
        assert!(!legacy.accepts_end(Some(SESSION)));
    }

    #[test]
    fn wrong_peer_stale_session_duplicate_and_late_resume_are_rejected() {
        let now = Instant::now();
        let mut recovery = active();
        recovery.disconnected(now);
        assert!(!recovery.restored(SESSION, "other", now));
        assert!(!recovery.restored(OTHER, "peer", now));
        assert!(!recovery.restored(SESSION, "peer", now + Duration::from_secs(45)));
        assert!(recovery.restored(SESSION, "peer", now + Duration::from_secs(1)));
        assert!(!recovery.restored(SESSION, "peer", now + Duration::from_secs(2)));
        assert_eq!(recovery.next_deadline(), None);
    }

    #[test]
    fn replacement_and_leave_cancel_old_recovery() {
        let now = Instant::now();
        let mut recovery = active();
        recovery.disconnected(now);
        recovery.begin("other");
        assert_eq!(recovery.session_for_login(now), None);
        assert!(!recovery.restored(SESSION, "peer", now));
        assert!(recovery.connected("other"));
        assert!(recovery.set_session(OTHER));
        recovery.disconnected(now);
        recovery.clear();
        assert_eq!(recovery.next_deadline(), None);
        assert_eq!(recovery.poll(now, true), RecoveryAction::Wait);
    }

    #[test]
    fn zero_remaining_time_ends_without_a_request() {
        let now = Instant::now();
        let mut recovery = active();
        assert!(recovery.suspended(SESSION, 0, now));
        assert_eq!(recovery.poll(now, true), RecoveryAction::End);
    }
}
