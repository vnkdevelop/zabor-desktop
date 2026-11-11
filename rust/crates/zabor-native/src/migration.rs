use crate::attestation::Attestation;
use crate::auth::{AuthClient, Credentials, LoginOutcome};
use crate::critical_intents::{CriticalIntents, IntentScope, IntentTicket};
use crate::hub::HubConnection;
use crate::profile::import::{self, ImportError, ImportOwner, PreparedImport, RecoveryAction};
use crate::profile::Profile;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;
use zabor_profile_helper::{
    Cancellation, ExportRequest, Failure, Limits, Observation, Operation, OwnerRegistry,
    TrustedFile,
};
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("invalid migration command or missing explicit approval; see docs/migration/native-import-command.md")]
    Approval,
    #[error("migration credentials unavailable or invalid; use environment input, never command arguments")]
    Credentials,
    #[error("independent account authentication failed")]
    Authentication,
    #[error("migration cancelled or original account intent revoked")]
    Cancelled,
    #[error("trusted profile helper failed or original resources could not be retired")]
    Helper,
    #[error(transparent)]
    Import(#[from] ImportError),
    #[error("published profile could not be reopened under the original account intent")]
    Profile,
}

enum Action {
    Import(Box<ImportSource>),
    Recover(RecoveryAction),
    Rollback,
}

struct ImportSource {
    source: PathBuf,
    scratch: PathBuf,
    runtime: PathBuf,
    helper: PathBuf,
    bundle: PathBuf,
    runtime_hash: [u8; 32],
    helper_hash: [u8; 32],
}

pub struct MigrationCommand {
    action: Action,
    parent: PathBuf,
    server: String,
    expected_key: Option<Value>,
}

pub fn is_command(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        matches!(
            argument.split('=').next().unwrap_or_default(),
            "--import-profile" | "--recover-profile" | "--rollback-profile"
        )
    })
}

impl MigrationCommand {
    pub fn parse(
        arguments: &[String],
        environment: impl Fn(&str) -> Option<Zeroizing<String>>,
    ) -> Result<Self, MigrationError> {
        let mut options = BTreeMap::new();
        let mut index = 0;
        while index < arguments.len() {
            let key = arguments[index].as_str();
            let value = if matches!(
                key,
                "--rollback-profile" | "--confirmed-key" | "--confirmed-no-identity"
            ) {
                ""
            } else {
                index += 1;
                arguments
                    .get(index)
                    .map(String::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or(MigrationError::Approval)?
            };
            if options.insert(key, value).is_some() {
                return Err(MigrationError::Approval);
            }
            index += 1;
        }
        let parent = PathBuf::from(options.remove("--parent").ok_or(MigrationError::Approval)?);
        let server = options
            .remove("--server")
            .ok_or(MigrationError::Approval)?
            .to_owned();
        if options.remove("--approved-server") != Some(server.as_str())
            || crate::hub::validate_endpoint(&server).is_err()
        {
            return Err(MigrationError::Approval);
        }
        let expected_key = match (
            options.remove("--confirmed-key"),
            options.remove("--confirmed-no-identity"),
        ) {
            (Some(_), None) => {
                let supplied = environment("ZABOR_PROFILE_EXPECTED_PUBLIC_KEY")
                    .ok_or(MigrationError::Approval)?;
                if supplied.len() > 4096 {
                    return Err(MigrationError::Approval);
                }
                let key: Value =
                    serde_json::from_str(&supplied).map_err(|_| MigrationError::Approval)?;
                if !key.as_object().is_some_and(|fields| {
                    fields.keys().all(|field| {
                        ["kty", "crv", "x", "y", "key_ops", "ext"].contains(&field.as_str())
                    })
                }) {
                    return Err(MigrationError::Approval);
                }
                crate::chat_crypto::ChatIdentity::generate()
                    .derive(&key)
                    .map_err(|_| MigrationError::Approval)?;
                Some(key)
            }
            (None, Some(_)) if environment("ZABOR_PROFILE_EXPECTED_PUBLIC_KEY").is_none() => None,
            _ => return Err(MigrationError::Approval),
        };
        let consent = options
            .remove("--consent")
            .ok_or(MigrationError::Approval)?;
        let action = match (
            options.remove("--import-profile"),
            options.remove("--recover-profile"),
            options.remove("--rollback-profile"),
        ) {
            (Some(source), None, None) if consent == "copy-import" => {
                Action::Import(Box::new(ImportSource {
                    source: source.into(),
                    scratch: options
                        .remove("--scratch")
                        .ok_or(MigrationError::Approval)?
                        .into(),
                    runtime: options
                        .remove("--runtime")
                        .ok_or(MigrationError::Approval)?
                        .into(),
                    helper: options
                        .remove("--helper")
                        .ok_or(MigrationError::Approval)?
                        .into(),
                    bundle: options
                        .remove("--approved-runtime-bundle")
                        .ok_or(MigrationError::Approval)?
                        .into(),
                    runtime_hash: approved_hash(environment(
                        "ZABOR_PROFILE_HELPER_TRUSTED_SHA256",
                    ))?,
                    helper_hash: approved_hash(environment("ZABOR_PROFILE_EXPORT_TRUSTED_SHA256"))?,
                }))
            }
            (None, Some(recovery), None) if consent == recovery => {
                Action::Recover(match recovery {
                    "publish-stage" => RecoveryAction::PublishVerifiedStage,
                    "confirm-published" => RecoveryAction::ConfirmPublished,
                    "abort-stage" => RecoveryAction::AbortUnpublishedStage,
                    "finish-rollback" => RecoveryAction::FinishRollback,
                    "finish-abort" => RecoveryAction::FinishAbortedPublication,
                    _ => return Err(MigrationError::Approval),
                })
            }
            (None, None, Some(_)) if consent == "rollback" => Action::Rollback,
            _ => return Err(MigrationError::Approval),
        };
        if !options.is_empty() || !parent.is_absolute() {
            return Err(MigrationError::Approval);
        }
        Ok(Self {
            action,
            parent,
            server,
            expected_key,
        })
    }
}

fn approved_hash(value: Option<Zeroizing<String>>) -> Result<[u8; 32], MigrationError> {
    let value = value.ok_or(MigrationError::Approval)?;
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(MigrationError::Approval);
    }
    let mut hash = [0; 32];
    for (index, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| MigrationError::Approval)?;
    }
    Ok(hash)
}

#[derive(Clone)]
pub struct MigrationCancellation {
    stop: CancellationToken,
    helper: Cancellation,
}

impl MigrationCancellation {
    pub fn cancel(&self) {
        self.stop.cancel();
        self.helper.cancel();
    }

    pub fn exporter_observation(&self) -> Result<Observation, MigrationError> {
        self.helper
            .observation()
            .map_err(|_| MigrationError::Helper)
    }
}

#[derive(Clone)]
pub struct MigrationRetirement {
    origin: Arc<()>,
    original: Observation,
    registry: Vec<Observation>,
    failure: Option<Failure>,
}

impl MigrationRetirement {
    pub fn exporter(&self) -> &Observation {
        &self.original
    }

    pub fn registry(&self) -> &[Observation] {
        &self.registry
    }

    pub fn failure(&self) -> Option<Failure> {
        self.failure.or(self.original.failure)
    }

    fn retired(&self) -> bool {
        fn observed(entry: &Observation) -> bool {
            if !entry.owner_started {
                return entry.pid.is_none() && !entry.job_created && !entry.reader_started;
            }
            entry.owner_joined
                && entry.handles_closed
                && (!entry.reader_started || entry.reader_joined)
                && (entry.pid.is_none() || entry.root_reaped && entry.job_empty)
        }
        self.failure.is_none()
            && observed(&self.original)
            && self.registry.iter().all(observed)
            && self
                .registry
                .iter()
                .any(|entry| entry.operation == self.original.operation)
    }

    fn exported(&self) -> bool {
        self.retired()
            && self.original.failure.is_none()
            && self.original.pid.is_some()
            && self.original.assigned
            && self.original.reader_started
            && self.original.pipe_eof
            && self.original.exit_code == Some(0)
    }
}

pub struct MigrationPublication {
    profile: Profile,
    ticket: IntentTicket,
    retirement: MigrationRetirement,
}

impl MigrationPublication {
    pub fn retirement(&self) -> &MigrationRetirement {
        &self.retirement
    }

    pub fn install<T>(
        self,
        operation: &MigrationOperation,
        install: impl FnOnce(Profile, IntentTicket) -> T,
    ) -> Result<T, MigrationError> {
        if !Arc::ptr_eq(&self.retirement.origin, &operation.origin)
            || self.ticket != operation.ticket
            || !self.retirement.retired()
            || !operation.current()
        {
            return Err(MigrationError::Cancelled);
        }
        operation
            .intents
            .commit(self.ticket, IntentScope::Session, || {
                if !operation.current_without_intent() {
                    return Err(MigrationError::Cancelled);
                }
                Ok(install(self.profile, self.ticket))
            })
            .ok_or(MigrationError::Cancelled)?
    }
}

pub struct MigrationOperation {
    command: MigrationCommand,
    intents: Arc<CriticalIntents>,
    ticket: IntentTicket,
    stop: CancellationToken,
    registry: OwnerRegistry,
    origin: Arc<()>,
    retirement: Mutex<Option<MigrationRetirement>>,
    helper: Option<Operation>,
    cancellation: MigrationCancellation,
    hub: Option<HubConnection>,
    monitor: Option<tokio::task::JoinHandle<()>>,
    owner: Option<ImportOwner>,
    user_id: Option<String>,
    credentials: Option<Credentials>,
}

struct HelperDrain<'a>(&'a OwnerRegistry);

impl Drop for HelperDrain<'_> {
    fn drop(&mut self) {
        let _ = self.0.drain();
    }
}

impl MigrationOperation {
    pub fn new(
        command: MigrationCommand,
        intents: Arc<CriticalIntents>,
        ticket: IntentTicket,
    ) -> Result<Self, MigrationError> {
        Self::with_registry(command, intents, ticket, OwnerRegistry::new())
    }

    pub fn with_registry(
        command: MigrationCommand,
        intents: Arc<CriticalIntents>,
        ticket: IntentTicket,
        registry: OwnerRegistry,
    ) -> Result<Self, MigrationError> {
        let stop = intents.session_token(ticket);
        let helper = registry.prepare().map_err(|_| MigrationError::Helper)?;
        let cancellation = MigrationCancellation {
            stop: stop.clone(),
            helper: helper.cancellation(),
        };
        Ok(Self {
            command,
            intents,
            ticket,
            stop,
            registry,
            origin: Arc::new(()),
            retirement: Mutex::new(None),
            helper: Some(helper),
            cancellation,
            hub: None,
            monitor: None,
            owner: None,
            user_id: None,
            credentials: None,
        })
    }

    pub fn cancellation(&self) -> MigrationCancellation {
        self.cancellation.clone()
    }

    pub fn retirement(&self) -> Option<MigrationRetirement> {
        self.retirement
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn current(&self) -> bool {
        !self.stop.is_cancelled()
            && self.intents.is_current(self.ticket, IntentScope::Session)
            && self.hub.as_ref().is_some_and(HubConnection::is_connected)
    }

    pub async fn authenticate(
        &mut self,
        credentials: Credentials,
        attestation: Option<&str>,
    ) -> Result<(), MigrationError> {
        if self.owner.is_some()
            || self.hub.is_some()
            || self.stop.is_cancelled()
            || !self.intents.is_current(self.ticket, IntentScope::Session)
        {
            return Err(MigrationError::Cancelled);
        }
        crate::auth::validate_credentials(&credentials).map_err(|_| MigrationError::Credentials)?;
        let (hub, mut events) = tokio::select! {
            _ = self.stop.cancelled() => return Err(MigrationError::Cancelled),
            result = HubConnection::connect(&self.command.server, attestation) => result.map_err(|_| MigrationError::Authentication)?,
        };
        self.hub = Some(hub.clone());
        let cancellation = self.cancellation.clone();
        self.monitor = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancellation.stop.cancelled() => { hub.close(); break; }
                    event = events.recv() => match event {
                        Some(event) if event.target != "ForceLogout" && event.target != "ClientRejected" => {}
                        _ => { cancellation.cancel(); hub.close(); break; }
                    }
                }
            }
        }));
        let auth = AuthClient::new(
            self.hub
                .as_ref()
                .ok_or(MigrationError::Authentication)?
                .clone(),
        );
        let outcome = tokio::select! {
            _ = self.stop.cancelled() => return Err(MigrationError::Cancelled),
            result = auth.login(&credentials) => result.map_err(|_| MigrationError::Authentication)?,
        };
        let LoginOutcome::SignedIn(bootstrap) = outcome else {
            return Err(MigrationError::Authentication);
        };
        if !self.current() || bootstrap.user.username != credentials.username {
            return Err(MigrationError::Authentication);
        }
        self.owner = Some(ImportOwner::verified(
            &bootstrap.user.id,
            &bootstrap.user.username,
            self.command.expected_key.clone(),
        )?);
        self.user_id = Some(bootstrap.user.id.clone());
        self.credentials = Some(credentials);
        Ok(())
    }

    pub fn execute(&mut self) -> Result<(), MigrationError> {
        self.execute_for_reuse().map(|_| ())
    }

    pub fn execute_for_reuse(&mut self) -> Result<Option<MigrationPublication>, MigrationError> {
        let owner = self.owner.as_ref().ok_or(MigrationError::Authentication)?;
        if !self.current() {
            return Err(MigrationError::Cancelled);
        }
        let accepting = match &self.command.action {
            Action::Import(config) => {
                let ImportSource {
                    source,
                    scratch,
                    runtime,
                    helper,
                    bundle,
                    runtime_hash,
                    helper_hash,
                } = &**config;
                let operation = self.helper.take().ok_or(MigrationError::Helper)?;
                let runtime = TrustedFile::pinned(runtime, *runtime_hash)
                    .map_err(|_| MigrationError::Helper)?;
                let helper = TrustedFile::pinned(helper, *helper_hash)
                    .map_err(|_| MigrationError::Helper)?;
                let approved_bundle = bundle
                    .canonicalize()
                    .map_err(|_| MigrationError::Approval)?;
                if runtime.path().parent() != Some(approved_bundle.as_path()) {
                    return Err(MigrationError::Approval);
                }
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| MigrationError::Approval)?
                    .as_millis();
                let now = i64::try_from(now).map_err(|_| MigrationError::Approval)?;
                let prepared = PreparedImport::copy(
                    source,
                    scratch,
                    &self.command.parent,
                    owner,
                    now,
                    |snapshot| {
                        let _drain = HelperDrain(&self.registry);
                        let exported = (|| {
                            if !self.current() {
                                return Err(ImportError::Export);
                            }
                            let request =
                                ExportRequest::copied_profile(runtime, helper, snapshot.root())
                                    .map_err(|_| ImportError::Export)?;
                            operation
                                .start(request, Limits::default())
                                .map_err(|_| ImportError::Export)?;
                            operation
                                .finish()
                                .map(|completed| completed.bytes)
                                .map_err(|_| ImportError::Export)
                        })();
                        self.retire_helpers().map_err(|_| ImportError::Export)?;
                        if exported.is_ok()
                            && !self.retirement().is_some_and(|receipt| receipt.exported())
                        {
                            return Err(ImportError::Export);
                        }
                        exported
                    },
                )?;
                if !prepared.publish(|| self.current())? {
                    return Err(MigrationError::Cancelled);
                }
                true
            }
            Action::Recover(action) => {
                self.retire_helpers()?;
                import::recover_authorized(&self.command.parent, *action, owner, || {
                    self.current()
                })?;
                matches!(
                    action,
                    RecoveryAction::PublishVerifiedStage | RecoveryAction::ConfirmPublished
                )
            }
            Action::Rollback => {
                self.retire_helpers()?;
                self.intents
                    .commit(self.ticket, IntentScope::Session, || {
                        if !self.current_without_intent() {
                            return Err(MigrationError::Cancelled);
                        }
                        import::rollback(&self.command.parent, owner)
                            .map(|_| ())
                            .map_err(Into::into)
                    })
                    .ok_or(MigrationError::Cancelled)??;
                false
            }
        };
        if accepting {
            if !self.current() {
                return Err(MigrationError::Cancelled);
            }
            let profile = Profile::open(&self.command.parent.join(import::BETA_PROFILE))
                .map_err(|_| MigrationError::Profile)?;
            let credentials = self
                .credentials
                .as_ref()
                .ok_or(MigrationError::Authentication)?;
            let saved = profile
                .load_session()
                .map_err(|_| MigrationError::Profile)?
                .ok_or(MigrationError::Profile)?;
            let user_id = self
                .user_id
                .as_ref()
                .ok_or(MigrationError::Authentication)?;
            if &saved.user_id != user_id || saved.username != credentials.username {
                return Err(MigrationError::Profile);
            }
            let session = profile
                .prepare_session(user_id, credentials)
                .map_err(|_| MigrationError::Profile)?;
            if !profile
                .commit_session_if(session, || self.current(), || self.stop.is_cancelled())
                .map_err(|_| MigrationError::Profile)?
            {
                return Err(MigrationError::Cancelled);
            }
            let retirement = self.retirement().ok_or(MigrationError::Helper)?;
            if !retirement.retired() || !self.current() {
                return Err(MigrationError::Cancelled);
            }
            return Ok(Some(MigrationPublication {
                profile,
                ticket: self.ticket,
                retirement,
            }));
        }
        Ok(None)
    }

    fn current_without_intent(&self) -> bool {
        !self.stop.is_cancelled() && self.hub.as_ref().is_some_and(HubConnection::is_connected)
    }

    fn retire_helpers(&self) -> Result<(), MigrationError> {
        let mut retained = self
            .retirement
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(receipt) = &*retained {
            return if receipt.retired() {
                Ok(())
            } else {
                Err(MigrationError::Helper)
            };
        }
        let drained = self.registry.drain();
        let original = self
            .cancellation
            .helper
            .observation()
            .map_err(|_| MigrationError::Helper)?;
        let receipt = MigrationRetirement {
            origin: self.origin.clone(),
            original,
            failure: drained.as_ref().err().copied(),
            registry: drained.unwrap_or_default(),
        };
        let retired = receipt.retired();
        *retained = Some(receipt);
        if retired {
            Ok(())
        } else {
            Err(MigrationError::Helper)
        }
    }

    pub async fn close(&mut self) -> Result<(), MigrationError> {
        self.cancellation.cancel();
        let retired = self.retire_helpers();
        if let Some(hub) = self.hub.take() {
            hub.close();
        }
        if let Some(monitor) = self.monitor.as_mut() {
            let joined = monitor.await;
            self.monitor.take();
            joined.map_err(|_| MigrationError::Helper)?;
        }
        self.credentials = None;
        retired
    }
}

impl Drop for MigrationOperation {
    fn drop(&mut self) {
        self.cancellation.cancel();
        let _ = self.registry.drain();
        if let Some(hub) = self.hub.take() {
            hub.close();
        }
        if let Some(monitor) = self.monitor.take() {
            monitor.abort();
        }
    }
}

pub fn run_cli(arguments: &[String]) -> Result<(), MigrationError> {
    let environment = |name: &str| std::env::var(name).ok().map(Zeroizing::new);
    let command = MigrationCommand::parse(arguments, environment)?;
    let username = environment("ZABOR_PROFILE_USERNAME").ok_or(MigrationError::Credentials)?;
    let password = environment("ZABOR_PROFILE_PASSWORD").ok_or(MigrationError::Credentials)?;
    let credentials = Credentials {
        username: username.as_str().to_owned(),
        password,
    };
    let attestation = Attestation::from_environment(crate::release::TARGET_BETA_VERSION)
        .map_err(|_| MigrationError::Approval)?
        .map(|signing| signing.token())
        .transpose()
        .map_err(|_| MigrationError::Approval)?;
    let intents = Arc::new(CriticalIntents::default());
    let ticket = intents.ticket();
    let mut operation = MigrationOperation::new(command, intents, ticket)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| MigrationError::Helper)?;
    let cancellation = operation.cancellation();
    let signal = runtime.spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        cancellation.cancel();
    });
    let result = runtime
        .block_on(operation.authenticate(credentials, attestation.as_deref().map(String::as_str)))
        .and_then(|()| operation.execute_for_reuse());
    let closed = runtime.block_on(operation.close());
    signal.abort();
    runtime.block_on(async {
        let _ = signal.await;
    });
    drop(result?);
    closed?;
    println!("Explicit native profile operation completed; original Electron profile unchanged.");
    Ok(())
}

#[cfg(test)]
#[path = "migration_tests.rs"]
mod tests;
