use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use zabor_native::chat_crypto::ChatIdentity;
use zabor_native::chat_store::ChatStore;
use zabor_native::profile::import::{
    rollback, ImportOwner, PreparedImport, Snapshot, BETA_PROFILE,
};
use zabor_native::profile::Profile;
use zabor_profile_helper::{ExportRequest, Limits, OwnerRegistry, TrustedFile};

fn hash(path: &Path) -> [u8; 32] {
    Sha256::digest(std::fs::read(path).unwrap()).into()
}

fn inventory(root: &Path) -> BTreeMap<PathBuf, [u8; 32]> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = BTreeMap::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.insert(path.strip_prefix(root).unwrap().to_path_buf(), hash(&path));
            }
        }
    }
    files
}

fn runtime(path: &Path) -> Result<TrustedFile, zabor_profile_helper::Failure> {
    if let Ok(value) = std::env::var("ZABOR_PROFILE_HELPER_TRUSTED_SHA256") {
        TrustedFile::pinned(path, approved_digest(&value)?)
    } else {
        TrustedFile::offline_signed(path)
    }
}

fn approved_digest(value: &str) -> Result<[u8; 32], zabor_profile_helper::Failure> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(zabor_profile_helper::Failure::Untrusted);
    }
    let mut digest = [0_u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| zabor_profile_helper::Failure::Untrusted)?;
    }
    Ok(digest)
}

fn approved_script(
    path: &Path,
    variable: &str,
) -> Result<TrustedFile, zabor_profile_helper::Failure> {
    let value = std::env::var(variable).map_err(|_| zabor_profile_helper::Failure::Untrusted)?;
    TrustedFile::pinned(path, approved_digest(&value)?)
}

fn fixture_public_key() -> serde_json::Value {
    let scalar = Sha256::digest(b"zabor-native-profile-synthetic-key-v1");
    let key = p256::SecretKey::from_slice(&scalar).unwrap();
    let mut public: serde_json::Value =
        serde_json::from_str(&key.public_key().to_jwk_string()).unwrap();
    public["key_ops"] = serde_json::json!([]);
    public["ext"] = serde_json::Value::Bool(true);
    public
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let executable = Path::new(&args[1]);
    if runtime(executable).is_err() {
        eprintln!("actual_chromium_roundtrip=BLOCKED_UNTRUSTED_RUNTIME");
        std::process::exit(2);
    }
    let scripts = Path::new(&args[2]);
    let seed = match approved_script(
        &scripts.join("native-profile-seed.cjs"),
        "ZABOR_PROFILE_SEED_TRUSTED_SHA256",
    ) {
        Ok(seed) => seed,
        Err(_) => {
            eprintln!("actual_chromium_roundtrip=BLOCKED_UNTRUSTED_SEED");
            std::process::exit(2);
        }
    };
    let helper = match approved_script(
        &scripts.join("native-profile-export.cjs"),
        "ZABOR_PROFILE_EXPORT_TRUSTED_SHA256",
    ) {
        Ok(helper) => helper,
        Err(_) => {
            eprintln!("actual_chromium_roundtrip=BLOCKED_UNTRUSTED_EXPORTER");
            std::process::exit(2);
        }
    };
    let public_key = fixture_public_key();
    let owner =
        ImportOwner::verified("fixture-owner", "fixture-login", Some(public_key.clone())).unwrap();
    let approved = Path::new(&args[3]);
    let source = tempfile::Builder::new()
        .prefix("zabor-electron-copy-")
        .tempdir_in(approved)
        .unwrap();
    let scratch = tempfile::Builder::new()
        .prefix("zabor-roundtrip-scratch-")
        .tempdir_in(approved)
        .unwrap();
    let parent = tempfile::Builder::new()
        .prefix("zabor-roundtrip-beta-")
        .tempdir_in(approved)
        .unwrap();
    std::fs::write(
        source.path().join(".native-copy-snapshot"),
        b"zabor-electron-copy-v1",
    )
    .unwrap();
    let registry = OwnerRegistry::new();
    let operation = registry.prepare().unwrap();
    operation
        .start(
            ExportRequest::synthetic_fixture(runtime(executable).unwrap(), seed, source.path())
                .unwrap(),
            Limits::default(),
        )
        .unwrap();
    let seeded = operation.finish().unwrap();
    assert!(
        seeded.observation.root_reaped
            && seeded.observation.job_empty
            && seeded.observation.reader_joined
            && seeded.observation.owner_joined
    );
    let metadata: serde_json::Value = serde_json::from_slice(&seeded.bytes).unwrap();
    assert!(metadata["fixture"] == true);
    let before = inventory(source.path());
    let snapshot = Snapshot::copy(source.path(), scratch.path()).unwrap();
    let operation = registry.prepare().unwrap();
    operation
        .start(
            ExportRequest::copied_profile(runtime(executable).unwrap(), helper, snapshot.root())
                .unwrap(),
            Limits::default(),
        )
        .unwrap();
    let exported = operation.finish().unwrap();
    assert!(
        exported.observation.root_reaped
            && exported.observation.job_empty
            && exported.observation.reader_joined
            && exported.observation.owner_joined
            && exported.observation.pipe_eof
    );
    snapshot.verify_source_unchanged().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let prepared =
        PreparedImport::prepare(&snapshot, parent.path(), &owner, &exported.bytes, now).unwrap();
    snapshot.close().unwrap();
    assert!(prepared.publish(|| true).unwrap());
    let root = parent.path().join(BETA_PROFILE);
    let profile = Profile::open(&root).unwrap();
    let session = profile.load_session().unwrap().unwrap();
    assert_eq!(session.user_id, "fixture-owner");
    assert!(session.password.as_str() == "fixture-private-only");
    assert_eq!(profile.read_settings().unwrap()["inputVolume"], 83);
    assert_eq!(profile.read_settings().unwrap()["relayOnly"], true);
    let identity = profile.load_identity().unwrap().unwrap();
    let restored = ChatIdentity::import(std::str::from_utf8(&identity).unwrap()).unwrap();
    assert!(restored.public_key().unwrap() == public_key);
    let store = ChatStore::open(&root.join("chat.sqlite3")).unwrap();
    assert_eq!(
        store
            .conversation("fixture-owner", "fixture-peer", now)
            .unwrap()
            .len(),
        2
    );
    assert!(store
        .conversation("foreign-owner", "fixture-peer", now)
        .unwrap()
        .is_empty());
    assert!(!store
        .accept_peer_key("fixture-owner", "fixture-peer", "changed")
        .unwrap());
    assert_eq!(
        store
            .deletions("fixture-owner", "fixture-peer", true)
            .unwrap(),
        vec!["fixture-deleted"]
    );
    assert_eq!(
        store
            .message("fixture-owner", "fixture-peer", "fixture-file", now)
            .unwrap()
            .unwrap()
            .file
            .unwrap()["savedPath"],
        serde_json::Value::Null
    );
    drop(store);
    drop(profile);
    rollback(parent.path(), &owner).unwrap();
    assert_eq!(inventory(source.path()), before);
    let observations = registry.drain().unwrap();
    assert!(observations
        .iter()
        .all(|observation| observation.owner_joined
            && observation.job_empty
            && observation.reader_joined));
    drop(exported);
    drop(seeded);
    source.close().unwrap();
    scratch.close().unwrap();
    parent.close().unwrap();
    println!(
        "actual_chromium_roundtrip=PASS source_inventory_unchanged=true owned_exits_and_joins=true"
    );
}
