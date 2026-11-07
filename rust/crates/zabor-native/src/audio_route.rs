use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceInventory {
    pub endpoints: Vec<String>,
    pub default: Option<String>,
}

impl DeviceInventory {
    pub fn resolve(&self, requested: &str) -> Option<String> {
        if !requested.is_empty()
            && requested != "default"
            && self.endpoints.iter().any(|id| id == requested)
        {
            return Some(requested.into());
        }
        self.default
            .as_ref()
            .filter(|id| self.endpoints.contains(id))
            .cloned()
    }
}

#[derive(Default)]
pub struct DeviceRecovery {
    lost_at: Option<Instant>,
    retry_at: Option<Instant>,
    failures: u32,
}

impl DeviceRecovery {
    pub const DEADLINE: Duration = Duration::from_secs(30);

    pub fn lost(&mut self, now: Instant) {
        self.lost_at.get_or_insert(now);
    }

    pub fn failed(&mut self, now: Instant) {
        self.lost(now);
        let delay = Duration::from_millis(250_u64 << self.failures.min(3));
        self.failures = self.failures.saturating_add(1);
        self.retry_at = Some(now + delay);
    }

    pub fn recovered(&mut self) {
        *self = Self::default();
    }

    pub fn can_retry(&self, now: Instant) -> bool {
        !self.expired(now) && self.retry_at.is_none_or(|deadline| now >= deadline)
    }

    pub fn expired(&self, now: Instant) -> bool {
        self.lost_at
            .is_some_and(|lost| now.saturating_duration_since(lost) >= Self::DEADLINE)
    }
}
