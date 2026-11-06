#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Downloading,
    Installing,
    Error,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    CancelDownload(u64),
    RetryDownload(u64),
    CloseWindow,
    None,
}

pub struct UpdateModel {
    pub phase: Phase,
    pub transferred: u64,
    pub total: u64,
    pub operation: u64,
}

impl UpdateModel {
    pub fn new(phase: Phase, transferred: u64, total: u64) -> Self {
        Self {
            phase,
            transferred,
            total,
            operation: 1,
        }
    }

    pub fn percent(&self) -> u8 {
        if self.total == 0 {
            return 0;
        }
        ((self.transferred as u128 * 100 + self.total as u128 / 2) / self.total as u128).min(100)
            as u8
    }

    pub fn progress(&mut self, operation: u64, transferred: u64, total: u64) -> bool {
        if self.phase != Phase::Downloading
            || operation != self.operation
            || transferred < self.transferred
        {
            return false;
        }
        self.transferred = transferred;
        self.total = total;
        true
    }

    pub fn close(&mut self) -> Effect {
        if self.phase == Phase::Closed {
            return Effect::None;
        }
        let effect = if self.phase == Phase::Downloading {
            Effect::CancelDownload(self.operation)
        } else {
            Effect::CloseWindow
        };
        self.phase = Phase::Closed;
        effect
    }

    pub fn action(&mut self) -> Effect {
        if self.phase == Phase::Error {
            let Some(operation) = self.operation.checked_add(1) else {
                return Effect::None;
            };
            self.operation = operation;
            self.phase = Phase::Downloading;
            self.transferred = 0;
            self.total = 0;
            Effect::RetryDownload(self.operation)
        } else if self.phase == Phase::Downloading {
            self.close()
        } else {
            Effect::None
        }
    }

    pub fn downloaded(&mut self, operation: u64) -> bool {
        if self.phase != Phase::Downloading || operation != self.operation {
            return false;
        }
        self.phase = Phase::Installing;
        true
    }

    pub fn failed(&mut self, operation: u64) -> bool {
        if !matches!(self.phase, Phase::Downloading | Phase::Installing)
            || operation != self.operation
        {
            return false;
        }
        self.phase = Phase::Error;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_download_rejects_late_progress_and_repeated_cancel() {
        let mut model = UpdateModel::new(Phase::Downloading, 42, 100);
        assert_eq!(model.close(), Effect::CancelDownload(1));
        assert!(!model.progress(1, 80, 100));
        assert_eq!(model.close(), Effect::None);
    }

    #[test]
    fn retry_rejects_previous_download_callbacks() {
        let mut model = UpdateModel::new(Phase::Error, 42, 100);
        assert_eq!(model.action(), Effect::RetryDownload(2));
        assert!(!model.progress(1, 100, 100));
        assert!(model.progress(2, 20, 100));
        assert!(!model.progress(2, 10, 100));
        assert_eq!(model.percent(), 20);
    }

    #[test]
    fn closing_installation_ui_does_not_cancel_the_installer() {
        let mut model = UpdateModel::new(Phase::Installing, 100, 100);
        assert_eq!(model.action(), Effect::None);
        assert_eq!(model.close(), Effect::CloseWindow);
    }

    #[test]
    fn progress_arithmetic_handles_unknown_total_and_large_values() {
        assert_eq!(UpdateModel::new(Phase::Downloading, 100, 0).percent(), 0);
        assert_eq!(
            UpdateModel::new(Phase::Downloading, u64::MAX, u64::MAX).percent(),
            100
        );
        assert_eq!(UpdateModel::new(Phase::Downloading, 3, 8).percent(), 38);
    }

    #[test]
    fn download_install_failure_retry_cannot_be_overwritten_by_stale_events() {
        let mut model = UpdateModel::new(Phase::Downloading, 0, 100);
        assert!(model.downloaded(1));
        assert!(!model.downloaded(1));
        assert!(!model.progress(1, 100, 100));
        assert!(model.failed(1));
        assert_eq!(model.action(), Effect::RetryDownload(2));
        assert!(!model.failed(1));
        assert!(!model.downloaded(1));
        assert_eq!(model.phase, Phase::Downloading);
        assert_eq!(model.close(), Effect::CancelDownload(2));
        assert!(!model.failed(2));
        assert!(!model.downloaded(2));
    }

    #[test]
    fn exhausted_operation_id_never_reuses_a_previous_download() {
        let mut model = UpdateModel::new(Phase::Error, 0, 0);
        model.operation = u64::MAX;
        assert_eq!(model.action(), Effect::None);
        assert_eq!(model.phase, Phase::Error);
        assert!(!model.progress(u64::MAX, 100, 100));
    }
}
