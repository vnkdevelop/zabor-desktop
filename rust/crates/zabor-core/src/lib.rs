pub const MAX_JS_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionEpoch(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EpochError {
    OutsideJavaScriptRange,
    Exhausted,
}

impl SessionEpoch {
    pub fn new(value: u64) -> Result<Self, EpochError> {
        if value > MAX_JS_SAFE_INTEGER {
            return Err(EpochError::OutsideJavaScriptRange);
        }
        Ok(Self(value))
    }

    pub fn value(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Result<Self, EpochError> {
        if self.0 == MAX_JS_SAFE_INTEGER {
            return Err(EpochError::Exhausted);
        }
        Ok(Self(self.0 + 1))
    }

    pub fn accepts(self, completion: Self) -> bool {
        self == completion
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_completion_does_not_match_after_stop_or_restart() {
        let original = SessionEpoch::new(0).unwrap();
        let stopped = original.next().unwrap();
        let restarted = stopped.next().unwrap();
        assert!(!restarted.accepts(original));
        assert!(!restarted.accepts(stopped));
        assert!(restarted.accepts(restarted));
    }

    #[test]
    fn js_integer_boundary_never_wraps_or_loses_precision() {
        let last = SessionEpoch::new(MAX_JS_SAFE_INTEGER).unwrap();
        assert_eq!(last.value(), MAX_JS_SAFE_INTEGER);
        assert_eq!(last.next(), Err(EpochError::Exhausted));
        assert_eq!(
            SessionEpoch::new(MAX_JS_SAFE_INTEGER + 1),
            Err(EpochError::OutsideJavaScriptRange)
        );
    }
}
