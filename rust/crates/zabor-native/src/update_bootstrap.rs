use super::{HostJob, UpdateHost};
use crate::update_helper::{
    CrashPoint, NativePlatform, Result, SignedBundle, StageContext, TrustPins, UpdateRoot,
};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

pub struct BootstrapSelection {
    pub root: Arc<UpdateRoot>,
    pub executable: PathBuf,
}

impl UpdateHost {
    pub(crate) fn startup_task<
        T: Send + 'static,
        F: Future<Output = Result<T>> + Send + 'static,
    >(
        &self,
        work: impl FnOnce(CancellationToken, Instant) -> F + Send + 'static,
    ) -> Result<HostJob<T>> {
        self.register(None, work)
    }

    pub fn bootstrap(
        &self,
        path: PathBuf,
        pins: TrustPins,
        platform: NativePlatform,
    ) -> Result<HostJob<BootstrapSelection>> {
        self.blocking(move |cancel, deadline| {
            if cancel.is_cancelled() || std::time::Instant::now() >= deadline {
                return Err("bootstrap-owner-cancelled-or-expired".into());
            }
            let root = UpdateRoot::open(&path, pins, platform)?;
            let executable = root.current_executable()?;
            if cancel.is_cancelled() || std::time::Instant::now() >= deadline {
                return Err("bootstrap-owner-cancelled-or-expired".into());
            }
            Ok(BootstrapSelection { root, executable })
        })
    }

    pub fn establish_baseline(
        &self,
        path: PathBuf,
        pins: TrustPins,
        platform: NativePlatform,
        bundle: SignedBundle,
        archive: PathBuf,
        now: u64,
    ) -> Result<HostJob<BootstrapSelection>> {
        let operation = self.operation();
        self.blocking(move |cancel, deadline| {
            let root = UpdateRoot::initialize_baseline_before(
                &path,
                pins,
                platform,
                bundle,
                &archive,
                StageContext {
                    operation,
                    cancellation: &cancel,
                    now,
                    deadline,
                    crash: CrashPoint::None,
                },
            )?;
            let executable = root.current_executable()?;
            if cancel.is_cancelled() || std::time::Instant::now() >= deadline {
                return Err("baseline-owner-cancelled-or-expired".into());
            }
            Ok(BootstrapSelection { root, executable })
        })
    }
}
