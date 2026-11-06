use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use tokio_util::sync::CancellationToken;
use zabor_native::chat::{ChatEngine, OutboundPacket};
use zabor_native::chat_crypto::ChatIdentity;
use zabor_native::chat_store::{ChatStore, RETENTION_MS};
use zabor_native::files::{encode_frame, save_verified, ChunkHeader, FileBroker};

const NOW: i64 = RETENTION_MS * 3;

fn engine(root: &Path, owner: &str, peer: &str) -> ChatEngine {
    let mut engine = ChatEngine::new(
        owner.into(),
        ChatIdentity::generate(),
        ChatStore::open(&root.join(format!("{owner}.sqlite"))).unwrap(),
    );
    engine.set_file_root(&root.join(owner)).unwrap();
    engine.set_friends([peer.into()]);
    engine
}

fn deliver(
    alice: &mut ChatEngine,
    bob: &mut ChatEngine,
    sender_alice: bool,
    packets: Vec<OutboundPacket>,
) {
    let mut queue: VecDeque<_> = packets
        .into_iter()
        .map(|packet| (sender_alice, packet))
        .collect();
    let mut count = 0;
    while let Some((from_alice, packet)) = queue.pop_front() {
        count += 1;
        assert!(count < 100);
        let replies = if from_alice {
            bob.receive("alice", &packet.payload, NOW).unwrap()
        } else {
            alice.receive("bob", &packet.payload, NOW).unwrap()
        };
        queue.extend(replies.into_iter().map(|packet| (!from_alice, packet)));
    }
}

fn metadata(alice: &mut ChatEngine, bob: &mut ChatEngine, root: &Path, bytes: &[u8]) -> String {
    let source = root.join("local document.bin");
    std::fs::write(&source, bytes).unwrap();
    let staged = alice.file_broker().unwrap().stage(&source).unwrap();
    let packets = alice.send_file("bob", staged, NOW).unwrap();
    deliver(alice, bob, true, packets);
    let messages = bob.messages("alice", NOW).unwrap();
    let message = messages.last().unwrap();
    assert_eq!(
        message.file.as_ref().unwrap()["storedName"],
        serde_json::Value::Null
    );
    assert_eq!(
        message.file.as_ref().unwrap()["savedPath"],
        serde_json::Value::Null
    );
    message.id.clone()
}

fn header(id: &str, transfer: &str, kind: &str, offset: Option<u64>) -> ChunkHeader {
    ChunkHeader {
        version: 1,
        sender_id: "alice".into(),
        target_id: "bob".into(),
        message_id: id.into(),
        transfer_id: transfer.into(),
        kind: kind.into(),
        offset,
    }
}

#[test]
fn requested_files_cancel_retry_verify_hash_and_preserve_read_receipts() {
    let root = tempfile::tempdir().unwrap();
    let mut alice = engine(root.path(), "alice", "bob");
    let mut bob = engine(root.path(), "bob", "alice");
    let bytes: Vec<u8> = (0..65536).map(|index| (index % 251) as u8).collect();
    let id = metadata(&mut alice, &mut bob, root.path(), &bytes);
    let request = bob.request_file("alice", &id, NOW).unwrap();
    deliver(&mut alice, &mut bob, false, vec![request]);
    assert_eq!(alice.take_file_requests(), vec![("bob".into(), id.clone())]);
    bob.receive_file(
        "alice",
        &encode_frame(&header(&id, "first", "file-begin", None), &[]).unwrap(),
        NOW,
    )
    .unwrap();
    bob.receive_file(
        "alice",
        &encode_frame(
            &header(&id, "first", "file-chunk", Some(0)),
            &bytes[..32000],
        )
        .unwrap(),
        NOW,
    )
    .unwrap();
    let cancel = bob.cancel_file("alice", &id, NOW).unwrap();
    deliver(&mut alice, &mut bob, false, vec![cancel]);
    assert_eq!(alice.take_file_cancels(), vec![("bob".into(), id.clone())]);
    assert_eq!(
        std::fs::read_dir(root.path().join("bob")).unwrap().count(),
        0
    );
    bob.request_file("alice", &id, NOW).unwrap();
    bob.receive_file(
        "alice",
        &encode_frame(&header(&id, "retry", "file-begin", None), &[]).unwrap(),
        NOW,
    )
    .unwrap();
    assert!(bob
        .receive_file(
            "alice",
            &encode_frame(
                &header(&id, "first", "file-chunk", Some(32000)),
                &bytes[32000..]
            )
            .unwrap(),
            NOW
        )
        .is_err());
    bob.receive_file(
        "alice",
        &encode_frame(&header(&id, "retry", "file-chunk", Some(0)), &bytes).unwrap(),
        NOW,
    )
    .unwrap();
    let receipts = bob.mark_read("alice", NOW).unwrap();
    deliver(&mut alice, &mut bob, false, receipts);
    assert!(
        bob.receive_file(
            "alice",
            &encode_frame(&header(&id, "retry", "file-complete", None), &[]).unwrap(),
            NOW
        )
        .unwrap()
        .2
    );
    let message = bob.messages("alice", NOW).unwrap().pop().unwrap();
    assert_eq!(message.delivery, "read");
    assert_eq!(message.file.as_ref().unwrap()["transferState"], "available");
    let (mut file, info) = bob.cached_file("alice", &id, NOW).unwrap();
    let mut received = Vec::new();
    file.read_to_end(&mut received).unwrap();
    assert_eq!(received, bytes);
    let target = root.path().join("user-chosen.bin");
    std::fs::write(&target, b"original destination").unwrap();
    let (file, _) = bob.cached_file("alice", &id, NOW).unwrap();
    assert!(save_verified(
        file,
        &target,
        info.size,
        &"0".repeat(64),
        &CancellationToken::new()
    )
    .is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"original destination");
    let (file, _) = bob.cached_file("alice", &id, NOW).unwrap();
    save_verified(
        file,
        &target,
        info.size,
        &info.sha256,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), bytes);
    assert_eq!(info.sha256, format!("{:x}", Sha256::digest(&bytes)));
    let unavailable = alice.unavailable("bob", &id).unwrap();
    deliver(&mut alice, &mut bob, true, vec![unavailable]);
    assert!(bob.cached_file("alice", &id, NOW).is_ok());
}

#[test]
fn corrupt_and_stalled_transfers_fail_without_leaving_partial_files() {
    let root = tempfile::tempdir().unwrap();
    let mut alice = engine(root.path(), "alice", "bob");
    let mut bob = engine(root.path(), "bob", "alice");
    let id = metadata(&mut alice, &mut bob, root.path(), b"correct");
    bob.request_file("alice", &id, NOW).unwrap();
    bob.receive_file(
        "alice",
        &encode_frame(&header(&id, "corrupt", "file-begin", None), &[]).unwrap(),
        NOW,
    )
    .unwrap();
    bob.receive_file(
        "alice",
        &encode_frame(&header(&id, "corrupt", "file-chunk", Some(0)), b"corrupt").unwrap(),
        NOW,
    )
    .unwrap();
    assert!(bob
        .receive_file(
            "alice",
            &encode_frame(&header(&id, "corrupt", "file-complete", None), &[]).unwrap(),
            NOW
        )
        .is_err());
    assert_eq!(
        std::fs::read_dir(root.path().join("bob")).unwrap().count(),
        0
    );
    assert!(!bob.has_downloads());
    assert_eq!(
        bob.messages("alice", NOW).unwrap()[0]
            .file
            .as_ref()
            .unwrap()["transferState"],
        "failed"
    );
    bob.request_file("alice", &id, NOW).unwrap();
    assert!(bob.expire_downloads(NOW + 29_999).is_empty());
    assert_eq!(bob.expire_downloads(NOW + 30_000).len(), 1);
    assert!(!bob.has_downloads());
    bob.request_file("alice", &id, NOW + 30_001).unwrap();
    bob.receive_file(
        "alice",
        &encode_frame(&header(&id, "revoked", "file-begin", None), &[]).unwrap(),
        NOW + 30_001,
    )
    .unwrap();
    bob.set_friends(Vec::new());
    assert_eq!(
        std::fs::read_dir(root.path().join("bob")).unwrap().count(),
        0
    );
    assert!(!bob.has_downloads());
}

#[test]
fn file_deletion_before_metadata_and_restart_cannot_resurrect_a_cache() {
    let root = tempfile::tempdir().unwrap();
    let mut alice = engine(root.path(), "alice", "bob");
    let mut bob = engine(root.path(), "bob", "alice");
    let key = alice.open("bob").unwrap();
    deliver(&mut alice, &mut bob, true, vec![key]);
    let source = root.path().join("source.bin");
    std::fs::write(&source, b"fixture").unwrap();
    let file = alice.file_broker().unwrap().stage(&source).unwrap();
    let packets = alice.send_file("bob", file, NOW).unwrap();
    let id = alice.messages("bob", NOW).unwrap()[0].id.clone();
    let deleted = alice.clear_file("bob", &id, NOW).unwrap();
    deliver(&mut alice, &mut bob, true, deleted);
    assert!(bob.messages("alice", NOW).unwrap().is_empty());
    deliver(&mut alice, &mut bob, true, packets);
    assert_eq!(
        bob.messages("alice", NOW).unwrap()[0]
            .file
            .as_ref()
            .unwrap()["transferState"],
        "deleted"
    );
    assert_eq!(
        std::fs::read_dir(root.path().join("alice"))
            .unwrap()
            .count(),
        0
    );
    let request = bob.request_file("alice", &id, NOW).unwrap();
    deliver(&mut alice, &mut bob, false, vec![request]);
    drop(bob);
    std::fs::write(
        root.path()
            .join("bob")
            .join(format!(".{}.part", "a".repeat(32))),
        b"abandoned",
    )
    .unwrap();
    std::fs::write(root.path().join("bob").join("user.part"), b"preserved").unwrap();
    let bob = engine(root.path(), "bob", "alice");
    assert_eq!(
        bob.messages("alice", NOW).unwrap()[0]
            .file
            .as_ref()
            .unwrap()["transferState"],
        "failed"
    );
    assert_eq!(
        std::fs::read_dir(root.path().join("bob")).unwrap().count(),
        1
    );
    assert_eq!(
        std::fs::read(root.path().join("bob/user.part")).unwrap(),
        b"preserved"
    );
    assert_eq!(
        bob.messages("alice", NOW).unwrap()[0]
            .file
            .as_ref()
            .unwrap()["storedName"],
        json!(null)
    );
}

#[test]
fn abandoned_and_cancelled_staging_preserve_the_source() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("original.bin");
    std::fs::write(&source, b"original").unwrap();
    let broker = FileBroker::new(&root.path().join("cache")).unwrap();
    let stop = CancellationToken::new();
    stop.cancel();
    assert!(broker.stage_cancellable(&source, &stop).is_err());
    assert_eq!(
        std::fs::read_dir(root.path().join("cache"))
            .unwrap()
            .count(),
        0
    );
    let staged = broker
        .stage_cancellable(&source, &CancellationToken::new())
        .unwrap();
    assert_eq!(
        std::fs::read_dir(root.path().join("cache"))
            .unwrap()
            .count(),
        1
    );
    drop(staged);
    assert_eq!(
        std::fs::read_dir(root.path().join("cache"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(std::fs::read(source).unwrap(), b"original");
}
