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

fn retry_saved_file(root: &Path, bytes: &[u8]) -> (ChatEngine, ChatStore, String) {
    let mut alice = engine(root, "alice", "bob");
    let mut bob = engine(root, "bob", "alice");
    let id = metadata(&mut alice, &mut bob, root, bytes);
    bob.request_file("alice", &id, NOW).unwrap();
    for (kind, payload, offset) in [
        ("file-begin", &[][..], None),
        ("file-chunk", bytes, Some(0)),
        ("file-complete", &[][..], None),
    ] {
        bob.receive_file(
            "alice",
            &encode_frame(&header(&id, "initial", kind, offset), payload).unwrap(),
            NOW,
        )
        .unwrap();
    }
    let (_, info) = bob.cached_file("alice", &id, NOW).unwrap();
    let target = root.join("previous-destination.bin");
    std::fs::write(&target, bytes).unwrap();
    bob.saved_file("alice", &id, &info.stored_name, &target, NOW)
        .unwrap();
    bob.file_broker()
        .unwrap()
        .delete(&info.stored_name)
        .unwrap();
    std::fs::remove_file(&target).unwrap();
    bob.request_file("alice", &id, NOW).unwrap();
    for (kind, payload, offset) in [
        ("file-begin", &[][..], None),
        ("file-chunk", &bytes[..2], Some(0)),
    ] {
        bob.receive_file(
            "alice",
            &encode_frame(&header(&id, "retry", kind, offset), payload).unwrap(),
            NOW,
        )
        .unwrap();
    }
    let store = ChatStore::open(&root.join("bob.sqlite")).unwrap();
    let registered = store.message("bob", "alice", &id, NOW).unwrap().unwrap();
    let registered = registered.file.unwrap();
    assert_eq!(registered["savedPath"], json!(target.to_string_lossy()));
    assert_eq!(registered["progress"], 0);
    assert!(
        bob.messages("alice", NOW).unwrap()[0]
            .file
            .as_ref()
            .unwrap()["progress"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    (bob, store, id)
}

#[test]
fn forget_saved_path_during_active_retry_preserves_valid_completion() {
    let root = tempfile::tempdir().unwrap();
    let bytes = b"verified retry";
    let (mut bob, store, id) = retry_saved_file(root.path(), bytes);
    bob.forget_saved_path("alice", &id, NOW).unwrap();
    bob.forget_saved_path("alice", &id, NOW).unwrap();
    bob.receive_file(
        "alice",
        &encode_frame(&header(&id, "retry", "file-chunk", Some(2)), &bytes[2..]).unwrap(),
        NOW,
    )
    .unwrap();
    assert!(
        bob.receive_file(
            "alice",
            &encode_frame(&header(&id, "retry", "file-complete", None), &[]).unwrap(),
            NOW
        )
        .unwrap()
        .2
    );
    assert!(!bob.has_downloads());
    let completed = store.message("bob", "alice", &id, NOW).unwrap().unwrap();
    let completed = completed.file.unwrap();
    assert_eq!(completed["savedPath"], json!(null));
    assert_eq!(completed["transferState"], "available");
    assert_eq!(completed["sourceAvailable"], true);
    assert_eq!(completed["progress"].as_f64(), Some(1.0));
    let (mut file, info) = bob.cached_file("alice", &id, NOW).unwrap();
    let mut received = Vec::new();
    file.read_to_end(&mut received).unwrap();
    assert_eq!(received, bytes);
    assert_eq!(info.sha256, format!("{:x}", Sha256::digest(bytes)));
}

#[test]
fn forget_saved_path_during_active_retry_allows_failed_transfer_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let bytes = b"verified retry";
    let (mut bob, store, id) = retry_saved_file(root.path(), bytes);
    bob.forget_saved_path("alice", &id, NOW).unwrap();
    let mut corrupt = bytes[2..].to_vec();
    corrupt[0] ^= 1;
    bob.receive_file(
        "alice",
        &encode_frame(&header(&id, "retry", "file-chunk", Some(2)), &corrupt).unwrap(),
        NOW,
    )
    .unwrap();
    assert!(bob
        .receive_file(
            "alice",
            &encode_frame(&header(&id, "retry", "file-complete", None), &[]).unwrap(),
            NOW
        )
        .is_err());
    assert!(!bob.has_downloads());
    assert_eq!(
        std::fs::read_dir(root.path().join("bob")).unwrap().count(),
        0
    );
    let failed = store.message("bob", "alice", &id, NOW).unwrap().unwrap();
    let failed = failed.file.unwrap();
    assert_eq!(failed["savedPath"], json!(null));
    assert_eq!(failed["storedName"], json!(null));
    assert_eq!(failed["transferState"], "failed");
    assert_eq!(failed["sourceAvailable"], false);
    assert_eq!(failed["progress"], 0);
    bob.request_file("alice", &id, NOW).unwrap();
}

#[test]
fn forget_saved_path_during_active_retry_rejects_replacement_or_deleted_message() {
    for replacement in ["file", "peer", "deleted"] {
        for fail in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let bytes = b"verified retry";
            let (mut bob, store, id) = retry_saved_file(root.path(), bytes);
            let mut changed = store.message("bob", "alice", &id, NOW).unwrap().unwrap();
            match replacement {
                "file" => {
                    let file = changed.file.as_mut().unwrap();
                    file["name"] = json!("replacement.bin");
                    file["savedPath"] = json!("replacement-destination.bin");
                    assert!(store.save(&changed, NOW).unwrap());
                }
                "peer" => {
                    changed.conversation_id = "replacement-peer".into();
                    assert!(store.save(&changed, NOW).unwrap());
                }
                "deleted" => store.delete_from_conversation("bob", "alice", &id).unwrap(),
                _ => unreachable!(),
            }
            bob.forget_saved_path("alice", &id, NOW).unwrap();
            let mut remaining = bytes[2..].to_vec();
            if fail {
                remaining[0] ^= 1;
            }
            bob.receive_file(
                "alice",
                &encode_frame(&header(&id, "retry", "file-chunk", Some(2)), &remaining).unwrap(),
                NOW,
            )
            .unwrap();
            assert!(bob
                .receive_file(
                    "alice",
                    &encode_frame(&header(&id, "retry", "file-complete", None), &[]).unwrap(),
                    NOW
                )
                .is_err());
            assert!(!bob.has_downloads());
            assert_eq!(
                std::fs::read_dir(root.path().join("bob")).unwrap().count(),
                0
            );
            if replacement == "deleted" {
                assert!(store.message("bob", "alice", &id, NOW).unwrap().is_none());
            } else {
                assert_eq!(
                    store
                        .message("bob", &changed.conversation_id, &id, NOW)
                        .unwrap()
                        .unwrap(),
                    changed
                );
            }
        }
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

#[test]
fn final_transfer_cannot_overwrite_a_concurrent_file_deletion_or_replacement() {
    for replacement in ["deleted", "available"] {
        let root = tempfile::tempdir().unwrap();
        let mut alice = engine(root.path(), "alice", "bob");
        let mut bob = engine(root.path(), "bob", "alice");
        let bytes = b"verified fixture";
        let id = metadata(&mut alice, &mut bob, root.path(), bytes);
        bob.request_file("alice", &id, NOW).unwrap();
        for (kind, payload, offset) in [
            ("file-begin", &[][..], None),
            ("file-chunk", &bytes[..], Some(0)),
        ] {
            bob.receive_file(
                "alice",
                &encode_frame(&header(&id, "original", kind, offset), payload).unwrap(),
                NOW,
            )
            .unwrap();
        }
        let store = ChatStore::open(&root.path().join("bob.sqlite")).unwrap();
        let mut changed = store
            .message("bob", "alice", &id, NOW)
            .unwrap()
            .unwrap()
            .file
            .unwrap();
        changed["transferState"] = json!(replacement);
        changed["storedName"] = json!("replacement.bin");
        assert!(store.set_file("bob", "alice", &id, &changed, NOW).unwrap());
        assert!(bob
            .receive_file(
                "alice",
                &encode_frame(&header(&id, "original", "file-complete", None), &[]).unwrap(),
                NOW
            )
            .is_err());
        assert!(!bob.has_downloads());
        assert_eq!(
            store
                .message("bob", "alice", &id, NOW)
                .unwrap()
                .unwrap()
                .file
                .unwrap(),
            changed
        );
        assert_eq!(
            std::fs::read_dir(root.path().join("bob")).unwrap().count(),
            0
        );
    }
}

#[test]
fn stale_saved_file_cannot_publish_a_path_for_deleted_replaced_expired_or_other_owner_data() {
    let root = tempfile::tempdir().unwrap();
    let mut alice = engine(root.path(), "alice", "bob");
    let mut bob = engine(root.path(), "bob", "alice");
    let bytes = b"verified fixture";
    let id = metadata(&mut alice, &mut bob, root.path(), bytes);
    bob.request_file("alice", &id, NOW).unwrap();
    for (kind, payload, offset) in [
        ("file-begin", &[][..], None),
        ("file-chunk", &bytes[..], Some(0)),
        ("file-complete", &[][..], None),
    ] {
        bob.receive_file(
            "alice",
            &encode_frame(&header(&id, "original", kind, offset), payload).unwrap(),
            NOW,
        )
        .unwrap();
    }
    let store = ChatStore::open(&root.path().join("bob.sqlite")).unwrap();
    let original = store.message("bob", "alice", &id, NOW).unwrap().unwrap();
    let name = original.file.as_ref().unwrap()["storedName"]
        .as_str()
        .unwrap();
    let target = root.path().join("user-chosen.bin");
    bob.saved_file("alice", &id, name, &target, NOW).unwrap();
    let mut saved = original.file.clone().unwrap();
    saved["savedPath"] = json!(target.to_string_lossy());
    assert_eq!(
        store
            .message("bob", "alice", &id, NOW)
            .unwrap()
            .unwrap()
            .file
            .unwrap(),
        saved
    );
    assert!(bob
        .saved_file("alice", &id, "other.bin", &target, NOW)
        .is_err());
    assert!(bob
        .saved_file("alice", &id, name, &target, NOW + RETENTION_MS)
        .is_err());
    let mut other = original.clone();
    other.owner_id = "other".into();
    assert!(store.save(&other, NOW).unwrap());
    let mut replaced = original.file.clone().unwrap();
    replaced["storedName"] = json!("replacement.bin");
    assert!(store.set_file("bob", "alice", &id, &replaced, NOW).unwrap());
    assert!(bob.saved_file("alice", &id, name, &target, NOW).is_err());
    let mut deleted = original.file.clone().unwrap();
    deleted["transferState"] = json!("deleted");
    assert!(store.set_file("bob", "alice", &id, &deleted, NOW).unwrap());
    assert!(bob.saved_file("alice", &id, name, &target, NOW).is_err());
    assert_eq!(
        store.message("other", "alice", &id, NOW).unwrap().unwrap(),
        other
    );
    assert_eq!(
        store
            .message("bob", "alice", &id, NOW)
            .unwrap()
            .unwrap()
            .file
            .unwrap(),
        deleted
    );
}

#[test]
fn split_friend_revocation_is_storage_free_until_original_cleanup_and_root_cannot_be_replaced() {
    let root = tempfile::tempdir().unwrap();
    let mut alice = engine(root.path(), "alice", "bob");
    let mut bob = engine(root.path(), "bob", "alice");
    let id = metadata(&mut alice, &mut bob, root.path(), b"fixture");
    bob.request_file("alice", &id, NOW).unwrap();
    bob.receive_file(
        "alice",
        &encode_frame(&header(&id, "original", "file-begin", None), &[]).unwrap(),
        NOW,
    )
    .unwrap();
    let store = ChatStore::open(&root.path().join("bob.sqlite")).unwrap();
    let revoked = bob.replace_friends(Vec::new());
    assert!(!bob.is_friend("alice"));
    assert!(!bob.has_downloads());
    assert_eq!(
        store
            .message("bob", "alice", &id, NOW)
            .unwrap()
            .unwrap()
            .file
            .unwrap()["transferState"],
        "transferring"
    );
    assert_eq!(
        std::fs::read_dir(root.path().join("bob")).unwrap().count(),
        1
    );
    assert!(bob.set_file_root(&root.path().join("replacement")).is_err());
    assert!(!root.path().join("replacement").exists());
    bob.finish_friend_revocations(revoked).unwrap();
    assert_eq!(
        std::fs::read_dir(root.path().join("bob")).unwrap().count(),
        0
    );
    assert_eq!(
        store
            .message("bob", "alice", &id, NOW)
            .unwrap()
            .unwrap()
            .file
            .unwrap()["transferState"],
        "failed"
    );
}

#[test]
fn delayed_friend_cleanup_keeps_the_new_original_transfer_even_with_reused_peer_message_and_transfer_id(
) {
    let root = tempfile::tempdir().unwrap();
    let mut alice = engine(root.path(), "alice", "bob");
    let mut bob = engine(root.path(), "bob", "alice");
    let bytes = b"fixture";
    let id = metadata(&mut alice, &mut bob, root.path(), bytes);
    bob.request_file("alice", &id, NOW).unwrap();
    let begin = encode_frame(&header(&id, "reused", "file-begin", None), &[]).unwrap();
    bob.receive_file("alice", &begin, NOW).unwrap();
    let revoked = bob.replace_friends(Vec::new());
    bob.set_friends(["alice".into()]);
    let packet = alice.open("bob").unwrap();
    deliver(&mut alice, &mut bob, true, vec![packet]);
    bob.request_file("alice", &id, NOW).unwrap();
    bob.receive_file("alice", &begin, NOW).unwrap();
    assert_eq!(
        std::fs::read_dir(root.path().join("bob")).unwrap().count(),
        2
    );
    bob.finish_friend_revocations(revoked).unwrap();
    assert_eq!(
        std::fs::read_dir(root.path().join("bob")).unwrap().count(),
        1
    );
    assert!(bob.is_downloading("alice", &id));
    let store = ChatStore::open(&root.path().join("bob.sqlite")).unwrap();
    assert_eq!(
        store
            .message("bob", "alice", &id, NOW)
            .unwrap()
            .unwrap()
            .file
            .unwrap()["transferState"],
        "transferring"
    );
    bob.receive_file(
        "alice",
        &encode_frame(&header(&id, "reused", "file-chunk", Some(0)), bytes).unwrap(),
        NOW,
    )
    .unwrap();
    assert!(
        bob.receive_file(
            "alice",
            &encode_frame(&header(&id, "reused", "file-complete", None), &[]).unwrap(),
            NOW
        )
        .unwrap()
        .2
    );
    assert_eq!(
        store
            .message("bob", "alice", &id, NOW)
            .unwrap()
            .unwrap()
            .file
            .unwrap()["transferState"],
        "available"
    );
}

#[test]
fn revocation_cleanup_rejects_a_replacement_engine_with_the_same_owner() {
    let root = tempfile::tempdir().unwrap();
    let mut alice = engine(root.path(), "alice", "bob");
    let mut bob = engine(root.path(), "bob", "alice");
    let id = metadata(&mut alice, &mut bob, root.path(), b"fixture");
    bob.request_file("alice", &id, NOW).unwrap();
    bob.receive_file(
        "alice",
        &encode_frame(&header(&id, "original", "file-begin", None), &[]).unwrap(),
        NOW,
    )
    .unwrap();
    let revoked = bob.replace_friends(Vec::new());
    let store = ChatStore::open(&root.path().join("bob.sqlite")).unwrap();
    let mut replacement = ChatEngine::new("bob".into(), ChatIdentity::generate(), store);
    assert!(replacement.finish_friend_revocations(revoked).is_err());
    assert_eq!(
        std::fs::read_dir(root.path().join("bob")).unwrap().count(),
        0
    );
    let store = ChatStore::open(&root.path().join("bob.sqlite")).unwrap();
    assert_eq!(
        store
            .message("bob", "alice", &id, NOW)
            .unwrap()
            .unwrap()
            .file
            .unwrap()["transferState"],
        "transferring"
    );
}
