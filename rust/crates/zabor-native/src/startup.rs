use crate::profile::Profile;
use crate::startup_route::StartupRoute;
use crate::update_helper::{
    HeldStartupPackage, NativePlatform, Result, SignedBundle, TrustPins, UpdateRoot,
};
use crate::update_host::process::HealthBootstrap;
use crate::update_host::{HostJob, OwnerReport, UpdateHost};
use p256::ecdsa::VerifyingKey;
use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub struct StartupConfig {
    pub root: PathBuf,
    pub helper: PathBuf,
    pub helper_sha256: String,
    pub pins: TrustPins,
}

fn provisioned(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("startup-trust-not-provisioned:{name}"))
}

#[allow(clippy::manual_is_multiple_of, clippy::chunks_exact_to_as_chunks)]
fn hex(value: &str) -> Result<Vec<u8>> {
    if value.len() % 2 != 0
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("startup-pin-encoding-invalid".into());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte: u8| {
                if byte.is_ascii_digit() {
                    byte - b'0'
                } else {
                    byte - b'a' + 10
                }
            };
            Ok(digit(pair[0]) * 16 + digit(pair[1]))
        })
        .collect()
}

impl StartupConfig {
    pub fn from_environment() -> Result<Self> {
        let pins = TrustPins {
            manifest_key: hex(&provisioned("ZABOR_NATIVE_MANIFEST_KEY_SEC1")?)?,
            acceptance_key: hex(&provisioned("ZABOR_NATIVE_ACCEPTANCE_KEY_SEC1")?)?,
            publisher_certificate_sha256: provisioned("ZABOR_NATIVE_PUBLISHER_CERTIFICATE_SHA256")?,
        };
        let config = Self {
            root: PathBuf::from(provisioned("ZABOR_NATIVE_INSTALL_ROOT")?),
            helper: PathBuf::from(provisioned("ZABOR_NATIVE_UPDATE_HELPER")?),
            helper_sha256: provisioned("ZABOR_NATIVE_UPDATE_HELPER_SHA256")?,
            pins,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if !self.root.is_absolute() || !self.helper.is_absolute() {
            return Err("startup-absolute-trusted-paths-required".into());
        }
        for key in [&self.pins.manifest_key, &self.pins.acceptance_key] {
            VerifyingKey::from_sec1_bytes(key).map_err(|_| "startup-public-key-invalid")?;
        }
        for hash in [&self.helper_sha256, &self.pins.publisher_certificate_sha256] {
            if hex(hash)?.len() != 32 || hash.bytes().all(|byte| byte == b'0') {
                return Err("startup-independent-pin-required".into());
            }
        }
        Ok(())
    }
}

fn gate(cancel: &CancellationToken, deadline: Instant) -> Result<()> {
    if cancel.is_cancelled() || Instant::now() >= deadline {
        Err("startup-original-owner-cancelled-or-expired".into())
    } else {
        Ok(())
    }
}

struct HealthState {
    bootstrap: Option<HealthBootstrap>,
    admitted: bool,
    outcome: Option<Result<()>>,
}

#[derive(Clone)]
pub struct ShellAdmission {
    host: Arc<UpdateHost>,
    execution: Arc<HeldStartupPackage>,
    health: Option<Arc<Mutex<HealthState>>>,
}

impl ShellAdmission {
    pub fn is_health_target(&self) -> bool {
        self.health.is_some()
    }

    pub fn local_ui_ready(
        &self,
        profile: Arc<Profile>,
        original_worker_ready: oneshot::Receiver<Result<()>>,
    ) -> Result<HostJob<()>> {
        let execution = self.execution.clone();
        let health = self.health.clone();
        if let Some(state) = &health {
            let mut state = state.lock().map_err(|_| "startup-health-state-poisoned")?;
            if state.admitted {
                return Err("startup-readiness-already-admitted".into());
            }
            state.admitted = true;
        }
        let failure_state = health.clone();
        let job = self.host.startup_task(move |cancel, deadline| async move {
            let result = async {
                gate(&cancel, deadline)?;
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err("startup-readiness-cancelled".into()),
                    _ = tokio::time::sleep_until(deadline.into()) => return Err("startup-readiness-expired".into()),
                    ready = original_worker_ready => ready.map_err(|_| "startup-original-worker-readiness-lost")??,
                }
                gate(&cancel, deadline)?;
                tokio::task::spawn_blocking(move || {
                    gate(&cancel, deadline)?;
                    let settings = profile.read_settings().map_err(|_| "startup-profile-settings-read-failed")?;
                    if !settings.is_object() {
                        return Err("startup-profile-settings-invalid".into());
                    }
                    if let Some(health) = &health {
                        let expected = Profile::default_path().map_err(|_| "startup-native-beta-profile-required")?
                            .canonicalize().map_err(|_| "startup-native-beta-profile-required")?;
                        if profile.root() != expected {
                            return Err("startup-native-beta-profile-binding-rejected".into());
                        }
                        let bootstrap = health.lock().map_err(|_| "startup-health-state-poisoned")?
                            .bootstrap.take().ok_or("startup-health-bootstrap-consumed")?;
                        gate(&cancel, deadline)?;
                        bootstrap.acknowledge_execution(&execution)?;
                    }
                    gate(&cancel, deadline)
                }).await.map_err(|_| "startup-original-readiness-owner-panicked")?
            }.await;
            if let Some(state) = failure_state {
                state.lock().map_err(|_| "startup-health-state-poisoned")?.outcome = Some(result.clone());
            }
            result
        });
        if let Err(failure) = &job {
            if let Some(state) = &self.health {
                state
                    .lock()
                    .map_err(|_| "startup-health-state-poisoned")?
                    .outcome = Some(Err(failure.clone()));
            }
        }
        job
    }
}

pub struct StartupOwner {
    runtime: Runtime,
    host: Arc<UpdateHost>,
    admission: ShellAdmission,
    _root: Option<Arc<UpdateRoot>>,
}

impl StartupOwner {
    pub fn prepare(route: &StartupRoute, config: StartupConfig) -> Result<Self> {
        config.validate()?;
        if !matches!(route, StartupRoute::App | StartupRoute::Health) {
            return Err("startup-app-or-health-route-required".into());
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|_| "startup-runtime-unavailable")?;
        let host = Arc::new(UpdateHost::new(1, Duration::from_secs(120))?);
        let health_target = *route == StartupRoute::Health;
        let result: crate::update_helper::Result<(
            Option<Arc<UpdateRoot>>,
            HeldStartupPackage,
            Option<HealthBootstrap>,
        )> = runtime.block_on(async {
            let helper = config.helper.clone();
            let hash = config.helper_sha256.clone();
            let publisher = config.pins.publisher_certificate_sha256.clone();
            let prerequisites = host
                .blocking(move |cancel, deadline| {
                    gate(&cancel, deadline)?;
                    let bootstrap = if health_target {
                        let stdin = std::io::stdin();
                        if stdin.is_terminal() {
                            return Err("private-inherited-health-stdin-required".into());
                        }
                        Some(HealthBootstrap::read(&mut stdin.lock())?)
                    } else {
                        None
                    };
                    gate(&cancel, deadline)?;
                    let platform = NativePlatform::pinned(&helper, &hash)?.certify(&publisher)?;
                    gate(&cancel, deadline)?;
                    Ok((platform, bootstrap))
                })?
                .wait()
                .await?;
            let (platform, bootstrap) = prerequisites;
            if let Some(bootstrap) = bootstrap {
                let execution = host
                    .blocking(move |cancel, deadline| {
                        gate(&cancel, deadline)?;
                        let executing = std::env::current_exe()
                            .map_err(|_| "startup-executing-image-unavailable")?;
                        let execution = HeldStartupPackage::open_pending(
                            &config.root,
                            config.pins,
                            platform,
                            &executing,
                        )?;
                        bootstrap.validate_execution(&execution)?;
                        gate(&cancel, deadline)?;
                        Ok((execution, bootstrap))
                    })?
                    .wait()
                    .await?;
                Ok((None, execution.0, Some(execution.1)))
            } else {
                let selected = host
                    .bootstrap(config.root, config.pins, platform)?
                    .wait()
                    .await?;
                let root = selected.root;
                let retained = root.clone();
                let execution = host
                    .blocking(move |cancel, deadline| {
                        gate(&cancel, deadline)?;
                        let executing = std::env::current_exe()
                            .map_err(|_| "startup-executing-image-unavailable")?;
                        let held = retained.hold_current_startup_package(&executing)?;
                        gate(&cancel, deadline)?;
                        Ok(held)
                    })?
                    .wait()
                    .await?;
                Ok((Some(root), execution, None))
            }
        });
        let (root, execution, bootstrap) = match result {
            Ok(result) => result,
            Err(error) => {
                let reports = runtime.block_on(host.drain())?;
                return Err(format!(
                    "{error}; original-startup-owner-reports:{}",
                    reports.len()
                ));
            }
        };
        let admission = ShellAdmission {
            host: host.clone(),
            execution: Arc::new(execution),
            health: bootstrap.map(|bootstrap| {
                Arc::new(Mutex::new(HealthState {
                    bootstrap: Some(bootstrap),
                    admitted: false,
                    outcome: None,
                }))
            }),
        };
        Ok(Self {
            runtime,
            host,
            admission,
            _root: root,
        })
    }

    pub fn admission(&self) -> ShellAdmission {
        self.admission.clone()
    }

    pub fn finish(
        self,
        application: std::result::Result<(), Box<dyn std::error::Error>>,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let reports = self.runtime.block_on(self.host.drain())?;
        let health = self
            .admission
            .health
            .as_ref()
            .map(|state| {
                state
                    .lock()
                    .map_err(|_| "startup-health-state-poisoned".to_owned())?
                    .outcome
                    .clone()
                    .unwrap_or_else(|| Err("startup-real-local-ui-readiness-not-observed".into()))
            })
            .transpose();
        let failures: Vec<&OwnerReport> = reports
            .iter()
            .filter(|report| report.result.is_err())
            .collect();
        application?;
        health?;
        if !failures.is_empty() {
            return Err(format!("startup-original-owner-failures:{failures:?}").into());
        }
        Ok(())
    }

    pub fn establish_baseline(
        config: StartupConfig,
        bundle: PathBuf,
        archive: PathBuf,
    ) -> Result<()> {
        config.validate()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|_| "startup-runtime-unavailable")?;
        let host = UpdateHost::new(1, Duration::from_secs(120))?;
        let result: crate::update_helper::Result<()> = runtime.block_on(async {
            let publisher = config.pins.publisher_certificate_sha256.clone();
            let prerequisite = host
                .blocking(move |cancel, deadline| {
                    gate(&cancel, deadline)?;
                    let mut file = crate::update_helper::open_read(&bundle)?;
                    let mut bytes = Vec::new();
                    Read::by_ref(&mut file)
                        .take(1024 * 1024 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| "baseline-bundle-read-failed")?;
                    if bytes.len() > 1024 * 1024 {
                        return Err("baseline-bundle-too-large".into());
                    }
                    let bundle: SignedBundle = serde_json::from_slice(&bytes)
                        .map_err(|_| "baseline-signed-bundle-invalid")?;
                    let platform = NativePlatform::pinned(&config.helper, &config.helper_sha256)?
                        .certify(&publisher)?;
                    gate(&cancel, deadline)?;
                    Ok((bundle, platform))
                })?
                .wait()
                .await?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "baseline-clock-invalid")?
                .as_secs();
            let selection = host
                .establish_baseline(
                    config.root,
                    config.pins,
                    prerequisite.1,
                    prerequisite.0,
                    archive,
                    now,
                )?
                .wait()
                .await?;
            drop(selection);
            Ok(())
        });
        let reports = runtime.block_on(host.drain())?;
        result?;
        if reports.iter().any(|report| report.result.is_err()) {
            return Err("baseline-original-owner-failed".into());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;
