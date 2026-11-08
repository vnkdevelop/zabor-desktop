use crate::release::{update_allowed, Channel, ReleaseGate, NATIVE_BETA_PROFILE};
use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MANIFEST_SIGNATURE_DOMAIN: &[u8] = b"ZABOR-NATIVE-UPDATE-V1\0";
pub const MAX_MANIFEST_BYTES: usize = 65_536;
pub const MAX_PACKAGE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeUpdateManifest {
    pub manifest_version: u32,
    pub version: String,
    pub channel: String,
    pub profile: String,
    pub platform: String,
    pub architecture: String,
    pub commit: String,
    pub sequence: u64,
    pub issued_at: u64,
    pub expires_at: u64,
    pub data_schema: u32,
    pub minimum_data_schema: u32,
    pub maximum_data_schema: u32,
    pub package_name: String,
    pub package_size: u64,
    pub package_sha256: String,
    pub inventory_sha256: String,
    pub publisher_certificate_sha256: String,
    pub acceptance_sha256: String,
}

#[derive(Clone, Debug)]
pub struct VerifiedRelease {
    manifest: NativeUpdateManifest,
    manifest_sha256: String,
}

impl VerifiedRelease {
    pub fn verify(
        raw: &[u8],
        signature: &[u8],
        trusted_public_key: &[u8],
        now: u64,
        minimum_sequence: u64,
    ) -> Result<Self, &'static str> {
        Self::verify_inner(
            raw,
            signature,
            trusted_public_key,
            Some(now),
            minimum_sequence,
        )
    }

    pub fn verify_retained(
        raw: &[u8],
        signature: &[u8],
        trusted_public_key: &[u8],
        expected_manifest_sha256: &str,
        highest_sequence: u64,
    ) -> Result<Self, &'static str> {
        let release = Self::verify_inner(raw, signature, trusted_public_key, None, 0)?;
        if release.manifest_sha256 != expected_manifest_sha256
            || release.manifest.sequence > highest_sequence
        {
            return Err("retained-receipt-mismatch");
        }
        Ok(release)
    }

    fn verify_inner(
        raw: &[u8],
        signature: &[u8],
        trusted_public_key: &[u8],
        now: Option<u64>,
        minimum_sequence: u64,
    ) -> Result<Self, &'static str> {
        if raw.is_empty() || raw.len() > MAX_MANIFEST_BYTES {
            return Err("invalid-manifest-length");
        }
        let key = VerifyingKey::from_sec1_bytes(trusted_public_key)
            .map_err(|_| "trusted-manifest-key-missing-or-invalid")?;
        let signature =
            Signature::from_slice(signature).map_err(|_| "invalid-manifest-signature")?;
        if signature.normalize_s().is_some() {
            return Err("noncanonical-manifest-signature");
        }
        let mut message = Vec::with_capacity(MANIFEST_SIGNATURE_DOMAIN.len() + raw.len());
        message.extend_from_slice(MANIFEST_SIGNATURE_DOMAIN);
        message.extend_from_slice(raw);
        key.verify(&message, &signature)
            .map_err(|_| "untrusted-manifest-signature")?;
        let manifest: NativeUpdateManifest =
            serde_json::from_slice(raw).map_err(|_| "invalid-manifest-json")?;
        let version = semver::Version::parse(&manifest.version).map_err(|_| "invalid-version")?;
        if manifest.manifest_version != 1
            || manifest.channel != "native-beta"
            || manifest.profile != NATIVE_BETA_PROFILE
            || manifest.platform != "win32"
            || manifest.architecture != "x64"
            || version.pre.as_str().split('.').next() != Some("rust")
            || !version.build.is_empty()
            || !hex_digest(&manifest.commit, 40)
            || manifest.sequence == 0
            || manifest.sequence < minimum_sequence
            || now.is_some_and(|now| manifest.issued_at > now || manifest.expires_at <= now)
            || manifest.expires_at <= manifest.issued_at
            || manifest.expires_at - manifest.issued_at > 7 * 24 * 60 * 60
            || !(1..=1000).contains(&manifest.data_schema)
            || manifest.minimum_data_schema == 0
            || manifest.minimum_data_schema > manifest.data_schema
            || manifest.maximum_data_schema < manifest.data_schema
            || manifest.maximum_data_schema > 1000
            || manifest.package_name != format!("ZABOR-Native-Beta-{}-x64.zip", manifest.version)
            || manifest.package_size == 0
            || manifest.package_size > MAX_PACKAGE_BYTES
            || !hex_digest(&manifest.package_sha256, 64)
            || !hex_digest(&manifest.inventory_sha256, 64)
            || !hex_digest(&manifest.publisher_certificate_sha256, 64)
            || !hex_digest(&manifest.acceptance_sha256, 64)
        {
            return Err("manifest-policy-rejected");
        }
        Ok(Self {
            manifest,
            manifest_sha256: format!("{:x}", Sha256::digest(raw)),
        })
    }

    pub fn manifest(&self) -> &NativeUpdateManifest {
        &self.manifest
    }

    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    pub fn fresh_at(&self, now: u64) -> bool {
        self.manifest.issued_at <= now && now < self.manifest.expires_at
    }

    pub fn permits_upgrade(&self, current: &str, data_schema: u32, last_sequence: u64) -> bool {
        self.manifest.sequence > last_sequence
            && data_schema >= self.manifest.minimum_data_schema
            && data_schema <= self.manifest.maximum_data_schema
            && update_allowed(
                current,
                &self.manifest.version,
                Channel::Beta,
                Channel::Beta,
            )
    }

    pub fn verify_package(&self, bytes: &[u8]) -> bool {
        bytes.len() as u64 == self.manifest.package_size
            && format!("{:x}", Sha256::digest(bytes)) == self.manifest.package_sha256
    }
}

fn hex_digest(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum TransactionPhase {
    Staged,
    AwaitingHealth,
    Committed,
    RollbackRequired,
    RolledBack,
}

#[derive(Debug)]
pub struct UpdateTransaction {
    operation: u64,
    target: VerifiedRelease,
    previous: VerifiedRelease,
    phase: TransactionPhase,
}

impl UpdateTransaction {
    pub fn stage(
        operation: u64,
        target: VerifiedRelease,
        previous: VerifiedRelease,
        data_schema: u32,
        last_sequence: u64,
    ) -> Result<Self, &'static str> {
        if operation == 0
            || !target.permits_upgrade(&previous.manifest.version, data_schema, last_sequence)
            || last_sequence < previous.manifest.sequence
            || data_schema != target.manifest.data_schema
            || data_schema != previous.manifest.data_schema
        {
            return Err("unsafe-update-or-schema-changing-rollback");
        }
        Ok(Self {
            operation,
            target,
            previous,
            phase: TransactionPhase::Staged,
        })
    }

    pub fn phase(&self) -> TransactionPhase {
        self.phase
    }

    pub fn activate(&mut self, operation: u64, gate: ReleaseGate, now: u64) -> bool {
        if operation != self.operation
            || self.phase != TransactionPhase::Staged
            || !self.target.fresh_at(now)
            || !gate.installation_allowed()
        {
            return false;
        }
        self.phase = TransactionPhase::AwaitingHealth;
        true
    }

    pub fn healthy(&mut self, operation: u64, manifest_sha256: &str) -> bool {
        if operation != self.operation
            || self.phase != TransactionPhase::AwaitingHealth
            || manifest_sha256 != self.target.manifest_sha256
        {
            return false;
        }
        self.phase = TransactionPhase::Committed;
        true
    }

    pub fn interrupted(&mut self, operation: u64) -> bool {
        if operation != self.operation
            || !matches!(
                self.phase,
                TransactionPhase::Staged | TransactionPhase::AwaitingHealth
            )
        {
            return false;
        }
        self.phase = TransactionPhase::RollbackRequired;
        true
    }

    pub fn rolled_back(&mut self, operation: u64, previous_manifest_sha256: &str) -> bool {
        if operation != self.operation
            || self.phase != TransactionPhase::RollbackRequired
            || previous_manifest_sha256 != self.previous.manifest_sha256
        {
            return false;
        }
        self.phase = TransactionPhase::RolledBack;
        true
    }

    pub fn retained_sequence(&self) -> u64 {
        self.target.manifest.sequence
    }
}

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
    use p256::ecdsa::{signature::Signer, SigningKey};

    fn fixture_key() -> SigningKey {
        loop {
            let mut bytes = [0_u8; 32];
            getrandom::fill(&mut bytes).unwrap();
            if let Ok(key) = SigningKey::from_slice(&bytes) {
                return key;
            }
        }
    }

    fn fixture_manifest(version: &str, sequence: u64) -> NativeUpdateManifest {
        let hash = format!("{:x}", Sha256::digest(b"LOCAL TEST FIXTURE ONLY"));
        NativeUpdateManifest {
            manifest_version: 1,
            version: version.into(),
            channel: "native-beta".into(),
            profile: NATIVE_BETA_PROFILE.into(),
            platform: "win32".into(),
            architecture: "x64".into(),
            commit: "1234567890abcdef1234567890abcdef12345678".into(),
            sequence,
            issued_at: 100,
            expires_at: 200,
            data_schema: 1,
            minimum_data_schema: 1,
            maximum_data_schema: 1,
            package_name: format!("ZABOR-Native-Beta-{version}-x64.zip"),
            package_size: 23,
            package_sha256: hash.clone(),
            inventory_sha256: hash.clone(),
            publisher_certificate_sha256: hash.clone(),
            acceptance_sha256: hash,
        }
    }

    fn sign_fixture(key: &SigningKey, raw: &[u8]) -> Signature {
        let mut message = MANIFEST_SIGNATURE_DOMAIN.to_vec();
        message.extend_from_slice(raw);
        let signature: Signature = key.sign(&message);
        signature.normalize_s().unwrap_or(signature)
    }

    fn verify_fixture(manifest: &NativeUpdateManifest) -> Result<VerifiedRelease, &'static str> {
        let key = fixture_key();
        let raw = serde_json::to_vec(manifest).unwrap();
        VerifiedRelease::verify(
            &raw,
            &sign_fixture(&key, &raw).to_bytes(),
            key.verifying_key().to_encoded_point(false).as_bytes(),
            150,
            1,
        )
    }

    #[test]
    fn signed_manifest_is_bound_to_exact_bytes_domain_and_pinned_key() {
        let key = fixture_key();
        let raw = serde_json::to_vec(&fixture_manifest("3.8.0-rust", 2)).unwrap();
        let signature = sign_fixture(&key, &raw).to_bytes();
        let public = key.verifying_key().to_encoded_point(false);
        let verified =
            VerifiedRelease::verify(&raw, &signature, public.as_bytes(), 150, 1).unwrap();
        assert_eq!(verified.manifest().version, "3.8.0-rust");
        assert_eq!(
            verified.manifest_sha256(),
            format!("{:x}", Sha256::digest(&raw))
        );
        let mut tampered = raw.clone();
        tampered.push(b' ');
        assert!(VerifiedRelease::verify(&tampered, &signature, public.as_bytes(), 150, 1).is_err());
        let other_key = fixture_key();
        assert!(VerifiedRelease::verify(
            &raw,
            &signature,
            other_key.verifying_key().to_encoded_point(false).as_bytes(),
            150,
            1
        )
        .is_err());
        assert!(VerifiedRelease::verify(&raw, &signature, &[], 150, 1).is_err());
        let no_domain: Signature = key.sign(&raw);
        assert!(
            VerifiedRelease::verify(&raw, &no_domain.to_bytes(), public.as_bytes(), 150, 1)
                .is_err()
        );
        assert!(VerifiedRelease::verify(&raw, &[], public.as_bytes(), 150, 1).is_err());
    }

    #[test]
    fn signed_but_wrong_channel_profile_architecture_or_path_fails_closed() {
        let base = fixture_manifest("3.8.0-rust", 2);
        for field in [
            "channel",
            "profile",
            "architecture",
            "platform",
            "packageName",
            "version",
        ] {
            let mut json = serde_json::to_value(&base).unwrap();
            json[field] = "../zabor-desktop-beta".into();
            let manifest: NativeUpdateManifest = serde_json::from_value(json).unwrap();
            assert!(verify_fixture(&manifest).is_err(), "{field}");
        }
        for version in ["3.8.0", "3.8.0-beta.rust", "3.8.0-rust+metadata"] {
            assert!(
                verify_fixture(&fixture_manifest(version, 2)).is_err(),
                "{version}"
            );
        }
    }

    #[test]
    fn signed_manifest_replay_expiry_and_resource_limits_are_enforced() {
        let key = fixture_key();
        let manifest = fixture_manifest("3.8.0-rust", 2);
        let raw = serde_json::to_vec(&manifest).unwrap();
        let signature = sign_fixture(&key, &raw).to_bytes();
        let public = key.verifying_key().to_encoded_point(false);
        for (now, sequence) in [(99, 1), (200, 1), (150, 3)] {
            assert!(
                VerifiedRelease::verify(&raw, &signature, public.as_bytes(), now, sequence)
                    .is_err()
            );
        }
        for size in [0, MAX_PACKAGE_BYTES + 1] {
            let mut manifest = manifest.clone();
            manifest.package_size = size;
            assert!(verify_fixture(&manifest).is_err());
        }
        let mut manifest = manifest;
        manifest.expires_at = manifest.issued_at + 7 * 24 * 60 * 60 + 1;
        assert!(verify_fixture(&manifest).is_err());
        assert!(
            VerifiedRelease::verify(&vec![b' '; MAX_MANIFEST_BYTES + 1], &[], &[], 150, 1).is_err()
        );
    }

    #[test]
    fn duplicate_or_unknown_signed_json_fields_are_rejected() {
        let key = fixture_key();
        let manifest = fixture_manifest("3.8.0-rust", 2);
        let raw = serde_json::to_string(&manifest).unwrap();
        for suffix in [r#", "channel":"native-beta"}"#, r#", "unexpected":true}"#] {
            let raw = format!("{}{suffix}", &raw[..raw.len() - 1]).into_bytes();
            assert!(VerifiedRelease::verify(
                &raw,
                &sign_fixture(&key, &raw).to_bytes(),
                key.verifying_key().to_encoded_point(false).as_bytes(),
                150,
                1
            )
            .is_err());
        }
    }

    #[test]
    fn verified_package_requires_both_length_and_digest() {
        let package = b"LOCAL TEST FIXTURE ONLY";
        let mut manifest = fixture_manifest("3.8.0-rust", 2);
        manifest.package_size = package.len() as u64;
        let verified = verify_fixture(&manifest).unwrap();
        assert!(verified.verify_package(package));
        assert!(!verified.verify_package(b"OTHER TEST FIXTURE BYTES"));
        assert!(!verified.verify_package(&package[..package.len() - 1]));
        assert!(verified.permits_upgrade("3.7.2-rust", 1, 1));
        assert!(!verified.permits_upgrade("3.8.0-rust", 1, 1));
        assert!(!verified.permits_upgrade("3.7.2-rust", 1, 2));
        assert!(!verified.permits_upgrade("3.7.2-rust", 2, 1));
        assert!(verified.fresh_at(150));
        assert!(!verified.fresh_at(99));
        assert!(!verified.fresh_at(200));
    }

    #[test]
    fn transaction_cannot_activate_from_local_checks_or_stale_operation() {
        let previous = verify_fixture(&fixture_manifest("3.7.2-rust", 1)).unwrap();
        let target = verify_fixture(&fixture_manifest("3.8.0-rust", 2)).unwrap();
        let previous_hash = previous.manifest_sha256().to_owned();
        let mut transaction = UpdateTransaction::stage(10, target, previous, 1, 1).unwrap();
        assert!(!transaction.activate(10, ReleaseGate::default(), 150));
        assert!(!transaction.activate(
            10,
            ReleaseGate {
                package_composition_verified: true,
                license_inventory_verified: true,
                ..ReleaseGate::default()
            },
            150
        ));
        assert!(!transaction.interrupted(9));
        assert_eq!(transaction.phase(), TransactionPhase::Staged);
        assert!(transaction.interrupted(10));
        assert!(!transaction.rolled_back(9, &previous_hash));
        assert!(!transaction.rolled_back(10, "wrong-hash"));
        assert!(transaction.rolled_back(10, &previous_hash));
        assert!(!transaction.interrupted(10));
        assert_eq!(transaction.retained_sequence(), 2);
    }

    #[test]
    fn health_confirmation_is_bound_to_original_operation_and_verified_target() {
        let previous = verify_fixture(&fixture_manifest("3.7.2-rust", 1)).unwrap();
        let target = verify_fixture(&fixture_manifest("3.8.0-rust", 2)).unwrap();
        let target_hash = target.manifest_sha256().to_owned();
        let mut transaction = UpdateTransaction::stage(10, target, previous, 1, 1).unwrap();
        assert!(!transaction.healthy(10, &target_hash));
        transaction.phase = TransactionPhase::AwaitingHealth;
        assert!(!transaction.healthy(9, &target_hash));
        assert!(!transaction.healthy(10, "wrong-hash"));
        assert!(transaction.healthy(10, &target_hash));
        assert!(!transaction.interrupted(10));
        assert!(!transaction.healthy(10, &target_hash));
        assert_eq!(transaction.phase(), TransactionPhase::Committed);
    }

    #[test]
    fn schema_changing_update_or_unowned_previous_release_is_not_rollback_safe() {
        let previous = verify_fixture(&fixture_manifest("3.7.2-rust", 1)).unwrap();
        let mut target = fixture_manifest("3.8.0-rust", 2);
        target.maximum_data_schema = 2;
        target.data_schema = 2;
        let target = verify_fixture(&target).unwrap();
        assert!(UpdateTransaction::stage(10, target, previous, 1, 1).is_err());
        let previous = verify_fixture(&fixture_manifest("3.7.2-rust", 1)).unwrap();
        let target = verify_fixture(&fixture_manifest("3.8.0-rust", 2)).unwrap();
        assert!(UpdateTransaction::stage(10, target.clone(), previous.clone(), 1, 0).is_err());
        assert!(UpdateTransaction::stage(0, target, previous, 1, 1).is_err());
    }

    #[test]
    fn rollback_retains_replay_fence_but_allows_a_newer_signed_retry() {
        let previous = verify_fixture(&fixture_manifest("3.7.2-rust", 1)).unwrap();
        let target = verify_fixture(&fixture_manifest("3.8.0-rust", 2)).unwrap();
        let previous_hash = previous.manifest_sha256().to_owned();
        let mut transaction =
            UpdateTransaction::stage(10, target.clone(), previous.clone(), 1, 1).unwrap();
        assert!(transaction.interrupted(10));
        assert!(transaction.rolled_back(10, &previous_hash));
        let last_sequence = transaction.retained_sequence();
        assert!(UpdateTransaction::stage(11, target, previous.clone(), 1, last_sequence).is_err());
        let retry = verify_fixture(&fixture_manifest("3.8.0-rust.1", 3)).unwrap();
        assert!(UpdateTransaction::stage(11, retry, previous, 1, last_sequence).is_ok());
    }

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
