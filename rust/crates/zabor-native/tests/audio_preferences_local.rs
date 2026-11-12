#![cfg(feature = "native-media")]

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use zabor_native::audio_preferences::{save_audio_settings, AudioPreferences};
use zabor_native::critical_intents::{CriticalAction, CriticalIntents, IntentScope};
use zabor_native::hub::{HubConnection, HubError};
use zabor_native::profile::Profile;
use zabor_native::protocol::{encode, Decoder};

struct Request {
    settings: Value,
    reply: oneshot::Sender<Value>,
}

struct Fixture {
    hub: HubConnection,
    requests: mpsc::Receiver<Request>,
    task: tokio::task::JoinHandle<()>,
    _events: mpsc::Receiver<zabor_native::hub::HubEvent>,
}

impl Fixture {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/zabor_v3", listener.local_addr().unwrap());
        let (sender, requests) = mpsc::channel(4);
        let task = tokio::spawn(async move {
            let (mut http, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                header.push(http.read_u8().await.unwrap());
                assert!(header.len() < 8192);
            }
            let body = json!({"connectionToken":"audio-preferences-fixture","availableTransports":[{"transport":"WebSockets","transferFormats":["Text"]}]}).to_string();
            http.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
            drop(http);
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap().into_text().unwrap(),
                "{\"protocol\":\"json\",\"version\":1}\u{1e}"
            );
            socket.send(Message::text("{}\u{1e}")).await.unwrap();
            let mut decoder = Decoder::default();
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                for request in decoder.push(text.as_bytes()).unwrap() {
                    if request["type"] == 6 {
                        continue;
                    }
                    assert_eq!(request["target"], "SaveAudioSettings");
                    let arguments = request["arguments"].as_array().unwrap();
                    assert_eq!(arguments.len(), 1);
                    let settings: Value =
                        serde_json::from_str(arguments[0].as_str().unwrap()).unwrap();
                    assert!(settings.is_object());
                    assert!(settings.get("nativeAudioSync").is_none());
                    assert!(settings.get("nativeDesktop").is_none());
                    let (reply, response) = oneshot::channel();
                    if sender.send(Request { settings, reply }).await.is_err() {
                        return;
                    }
                    let Ok(result) = response.await else {
                        return;
                    };
                    if socket.send(Message::text(encode(&json!({"type":3,"invocationId":request["invocationId"],"result":result})).unwrap())).await.is_err() { return; }
                }
            }
        });
        let (hub, events) = HubConnection::connect(&endpoint, None).await.unwrap();
        Self {
            hub,
            requests,
            task,
            _events: events,
        }
    }

    async fn request(&mut self) -> Request {
        timeout(Duration::from_secs(3), self.requests.recv())
            .await
            .unwrap()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.hub.close();
        self.task.abort();
    }
}

fn no_failure() -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(|| panic!("settings fixture unexpectedly failed"))
}

#[tokio::test]
async fn latest_wins_and_local_flush_do_not_wait_for_a_slow_server_save() {
    timeout(Duration::from_secs(8), async {
        let temporary = tempfile::tempdir().unwrap();
        let profile = Arc::new(Profile::open(temporary.path()).unwrap());
        let mut fixture = Fixture::new().await;
        let preferences =
            AudioPreferences::new(&json!({"language":"en","futureLocal":{"enabled":true}}));
        assert!(preferences.begin_session(
            "alice",
            fixture.hub.clone(),
            Some(&json!({"inputVolume":75,"futureServer":true}))
        ));
        let remote_stop = CancellationToken::new();
        let remote = tokio::spawn(
            preferences
                .clone()
                .run_remote(remote_stop.clone(), no_failure()),
        );
        let local_stop = CancellationToken::new();
        let local = tokio::spawn(preferences.clone().run_local(
            Some(profile.clone()),
            local_stop.clone(),
            no_failure(),
        ));
        assert!(preferences.edit("alice", "inputVolume", json!(125)));
        let first = fixture.request().await;
        assert_eq!(first.settings["inputVolume"].as_f64(), Some(125.0));
        for volume in 0..4096 {
            assert!(preferences.edit("alice", "inputVolume", json!(volume % 201)));
        }
        assert!(preferences.edit("alice", "inputVolume", json!(175)));
        assert!(preferences.edit("alice", "outputVolume", json!(200)));
        assert!(preferences.language("ru"));
        assert!(preferences.peer_volume("alice", "bob", 160.0));
        assert_eq!(
            preferences.controls.settings.lock().unwrap().input_volume,
            175.0
        );
        local_stop.cancel();
        timeout(Duration::from_secs(2), local)
            .await
            .unwrap()
            .unwrap();
        let stored = profile.read_settings().unwrap();
        assert_eq!(stored["inputVolume"].as_f64(), Some(175.0));
        assert_eq!(stored["outputVolume"].as_f64(), Some(200.0));
        assert_eq!(stored["language"], "ru");
        assert_eq!(
            stored["nativeAudioSync"],
            json!({"owner":"alice","pending":true})
        );
        assert_eq!(stored["futureLocal"]["enabled"], true);
        assert_eq!(stored["futureServer"], true);
        first.reply.send(Value::Null).unwrap();
        let latest = fixture.request().await;
        assert_eq!(latest.settings["inputVolume"].as_f64(), Some(175.0));
        assert_eq!(latest.settings["outputVolume"].as_f64(), Some(200.0));
        assert_eq!(latest.settings["language"], "ru");
        assert_eq!(latest.settings["userVolumes"]["bob"].as_f64(), Some(160.0));
        latest.reply.send(Value::Null).unwrap();
        assert!(timeout(Duration::from_millis(650), fixture.requests.recv())
            .await
            .is_err());
        remote_stop.cancel();
        remote.await.unwrap();
        let flush_stop = CancellationToken::new();
        flush_stop.cancel();
        preferences
            .clone()
            .run_local(Some(profile.clone()), flush_stop, no_failure())
            .await;
        assert_eq!(
            profile.read_settings().unwrap()["nativeAudioSync"]["pending"],
            false
        );
        assert_eq!(
            std::fs::read_dir(temporary.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
                .count(),
            0
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn rejected_save_is_bounded_and_survives_reconnect_and_process_restore_for_the_same_owner() {
    timeout(Duration::from_secs(8), async {
        let mut old = Fixture::new().await;
        let mut new = Fixture::new().await;
        let preferences = AudioPreferences::new(&json!({"language":"en"}));
        assert!(preferences.begin_session("alice", old.hub.clone(), None));
        let failures = Arc::new(AtomicUsize::new(0));
        let observed = failures.clone();
        let stop = CancellationToken::new();
        let remote = tokio::spawn(preferences.clone().run_remote(stop.clone(), Arc::new(move || { observed.fetch_add(1, Ordering::AcqRel); })));
        assert!(preferences.edit("alice", "selectedInput", json!("browser-hashed-original")));
        old.request().await.reply.send(json!(false)).unwrap();
        assert!(timeout(Duration::from_millis(700), old.requests.recv()).await.is_err());
        assert_eq!(failures.load(Ordering::Acquire), 1);
        preferences.disconnect();
        assert!(preferences.language("ru"));
        assert!(preferences.edit("alice", "outputVolume", json!(150)));
        let temporary = tempfile::tempdir().unwrap();
        let profile = Arc::new(Profile::open(temporary.path()).unwrap());
        let flush = CancellationToken::new();
        flush.cancel();
        preferences.clone().run_local(Some(profile.clone()), flush, no_failure()).await;
        let restored = AudioPreferences::new(&profile.read_settings().unwrap());
        assert!(restored.begin_session("alice", new.hub.clone(), Some(&json!({"selectedInput":"default","outputVolume":20,"language":"en","addedServerField":true}))));
        assert_eq!(restored.settings().unwrap().selected_input, "browser-hashed-original");
        assert_eq!(restored.settings().unwrap().output_volume, 150.0);
        assert_eq!(restored.settings().unwrap().language, "ru");
        assert_eq!(restored.settings().unwrap().extra["addedServerField"], true);
        let resumed_stop = CancellationToken::new();
        let resumed = tokio::spawn(restored.run_remote(resumed_stop.clone(), no_failure()));
        let request = new.request().await;
        assert_eq!(request.settings["selectedInput"], "browser-hashed-original");
        assert_eq!(request.settings["outputVolume"].as_f64(), Some(150.0));
        request.reply.send(Value::Null).unwrap();
        resumed_stop.cancel();
        resumed.await.unwrap();
        stop.cancel();
        remote.await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn stale_save_completion_cannot_acknowledge_a_new_owner_or_send_old_pending_settings() {
    timeout(Duration::from_secs(8), async {
        let mut old = Fixture::new().await;
        let mut new = Fixture::new().await;
        let preferences = AudioPreferences::new(&json!({"language":"en"}));
        preferences.begin_session("alice", old.hub.clone(), None);
        let stop = CancellationToken::new();
        let remote = tokio::spawn(preferences.clone().run_remote(stop.clone(), no_failure()));
        assert!(preferences.peer_volume("alice", "old-friend", 200.0));
        assert!(preferences.edit("alice", "inputVolume", json!(180)));
        let pending = old.request().await;
        preferences.logout();
        assert!(!preferences.edit("alice", "inputVolume", json!(190)));
        assert!(preferences.begin_session(
            "bob",
            new.hub.clone(),
            Some(&json!({"inputVolume":30,"userVolumes":{"new-friend":80}}))
        ));
        assert!(!preferences.edit("alice", "outputVolume", json!(190)));
        assert!(!preferences
            .settings()
            .unwrap()
            .user_volumes
            .contains_key("old-friend"));
        assert_eq!(preferences.settings().unwrap().input_volume, 30.0);
        assert!(preferences.edit("bob", "outputVolume", json!(140)));
        pending.reply.send(Value::Null).unwrap();
        let latest = new.request().await;
        assert_eq!(latest.settings["inputVolume"].as_f64(), Some(30.0));
        assert_eq!(latest.settings["outputVolume"].as_f64(), Some(140.0));
        assert_eq!(latest.settings["userVolumes"], json!({"new-friend":80.0}));
        latest.reply.send(Value::Null).unwrap();
        stop.cancel();
        remote.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn wire_contract_uses_a_json_string_and_distinguishes_null_false_and_malformed_success() {
    timeout(Duration::from_secs(5), async {
        let mut fixture = Fixture::new().await;
        for (result, expected) in [
            (Value::Null, Ok(true)),
            (json!(true), Ok(true)),
            (json!(false), Ok(false)),
            (json!("success"), Err(HubError::Protocol)),
        ] {
            let hub = fixture.hub.clone();
            let save =
                tokio::spawn(
                    async move { save_audio_settings(&hub, "{\"inputVolume\":75}").await },
                );
            let request = fixture.request().await;
            assert_eq!(request.settings, json!({"inputVolume":75}));
            request.reply.send(result).unwrap();
            assert_eq!(save.await.unwrap(), expected);
        }
        for invalid in ["[]", "null", "not-json"] {
            assert_eq!(
                save_audio_settings(&fixture.hub, invalid).await,
                Err(HubError::Protocol)
            );
        }
        assert_eq!(
            save_audio_settings(&fixture.hub, &" ".repeat(1024 * 1024 + 1)).await,
            Err(HubError::Protocol)
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn server_fields_cannot_inject_local_sync_metadata_or_overwrite_local_language() {
    let fixture = Fixture::new().await;
    let preferences = AudioPreferences::new(&json!({"language":"en","future":{"enabled":true}}));
    assert!(!preferences.edit("alice", "inputVolume", json!(175)));
    assert!(preferences.begin_session("alice", fixture.hub.clone(), Some(&json!({"language":"ru","inputVolume":50,"nativeAudioSync":{"owner":"other","pending":true}}))));
    let settings = preferences.settings().unwrap();
    assert_eq!(settings.language, "en");
    assert_eq!(settings.input_volume, 50.0);
    assert!(!settings.extra.contains_key("nativeAudioSync"));
    assert_eq!(settings.extra["future"]["enabled"], true);
    let revision = preferences
        .controls
        .settings_revision
        .load(Ordering::Acquire);
    assert!(!preferences.edit("alice", "noiseSuppression", json!("false")));
    assert!(!preferences.edit("alice", "unknown", json!(false)));
    assert_eq!(
        preferences
            .controls
            .settings_revision
            .load(Ordering::Acquire),
        revision
    );
    assert!(preferences.edit("alice", "relayOnly", json!(true)));
    assert!(preferences.relay_only());
}

#[tokio::test]
async fn prepared_audio_restore_preserves_legacy_merge_dirty_owner_and_device_epochs() {
    let fixture = Fixture::new().await;
    let original = json!({"language":"en","futureLocal":true,"selectedInput":"retained-device"});
    let legacy = AudioPreferences::new(&original);
    let guarded = AudioPreferences::new(&original);
    for preferences in [&legacy, &guarded] {
        assert!(preferences.begin_session("alice", fixture.hub.clone(), None));
        assert!(preferences.edit("alice", "inputVolume", json!(175)));
        assert!(preferences.peer_volume("alice", "old-peer", 180.0));
    }
    for (owner, server) in [
        (
            "alice",
            json!({"language":"ru","inputVolume":30,"selectedInput":"default","futureServer":true}),
        ),
        (
            "bob",
            json!({"inputVolume":45,"selectedInput":"next-device","selectedOutput":"next-output","userVolumes":{"new-peer":90},"nativeAudioSync":{"owner":"injected","pending":true}}),
        ),
    ] {
        assert!(legacy.begin_session(owner, fixture.hub.clone(), Some(&server)));
        {
            let mut prepared = guarded
                .try_prepare_session(owner, fixture.hub.clone(), Some(&server))
                .unwrap()
                .unwrap();
            prepared.apply();
            prepared.apply();
        }
        assert_eq!(guarded.settings(), legacy.settings());
        let expected = &legacy.controls;
        let actual = &guarded.controls;
        for (actual, expected) in [
            (&actual.settings_revision, &expected.settings_revision),
            (&actual.input_device_epoch, &expected.input_device_epoch),
            (&actual.output_device_epoch, &expected.output_device_epoch),
            (&actual.capture_epoch, &expected.capture_epoch),
            (&actual.playback_epoch, &expected.playback_epoch),
        ] {
            assert_eq!(
                actual.load(Ordering::Acquire),
                expected.load(Ordering::Acquire)
            );
        }
    }
    for preferences in [&legacy, &guarded] {
        let mut controls_only = preferences.settings().unwrap();
        controls_only.input_volume = 7.0;
        assert!(preferences.controls.apply_settings(controls_only));
    }
    assert!(legacy.begin_session("bob", fixture.hub.clone(), None));
    {
        let mut prepared = guarded
            .try_prepare_session("bob", fixture.hub.clone(), None)
            .unwrap()
            .unwrap();
        prepared.apply();
    }
    assert_eq!(
        guarded.controls.settings.lock().unwrap().input_volume,
        legacy.controls.settings.lock().unwrap().input_volume
    );
    assert_eq!(
        guarded.controls.settings_revision.load(Ordering::Acquire),
        legacy.controls.settings_revision.load(Ordering::Acquire)
    );
    assert!(!guarded.edit("alice", "outputVolume", json!(150)));
    assert!(guarded.edit("bob", "outputVolume", json!(150)));
    assert_eq!(guarded.settings().unwrap().language, "en");
    assert!(!guarded
        .settings()
        .unwrap()
        .user_volumes
        .contains_key("old-peer"));
    assert!(!guarded
        .settings()
        .unwrap()
        .extra
        .contains_key("nativeAudioSync"));
}

#[tokio::test]
async fn discarded_or_revoked_prepared_restore_does_not_cancel_or_acknowledge_previous_save() {
    timeout(Duration::from_secs(8), async {
        let mut fixture = Fixture::new().await;
        let preferences = AudioPreferences::new(&json!({"language":"en"}));
        assert!(preferences.begin_session("alice", fixture.hub.clone(), None));
        assert!(preferences.edit("alice", "inputVolume", json!(175)));
        let stop = CancellationToken::new();
        let remote = tokio::spawn(preferences.clone().run_remote(stop.clone(), no_failure()));
        let pending = fixture.request().await;
        let original = preferences.settings().unwrap();
        let revision = preferences
            .controls
            .settings_revision
            .load(Ordering::Acquire);
        for revoked in [false, true] {
            let intents = CriticalIntents::default();
            let ticket = intents.ticket();
            let mut prepared = preferences
                .try_prepare_session("bob", fixture.hub.clone(), Some(&json!({"inputVolume":30})))
                .unwrap()
                .unwrap();
            if revoked {
                intents.request(CriticalAction::Logout);
                assert!(intents
                    .commit(ticket, IntentScope::Session, || prepared.apply())
                    .is_none());
            }
            drop(prepared);
            assert_eq!(preferences.settings().unwrap(), original);
            assert_eq!(
                preferences
                    .controls
                    .settings_revision
                    .load(Ordering::Acquire),
                revision
            );
        }
        pending.reply.send(Value::Null).unwrap();
        assert!(timeout(Duration::from_millis(650), fixture.requests.recv())
            .await
            .is_err());
        assert!(preferences.edit("alice", "outputVolume", json!(140)));
        let latest = fixture.request().await;
        assert_eq!(latest.settings["inputVolume"], json!(175.0));
        assert_eq!(latest.settings["outputVolume"], json!(140.0));
        latest.reply.send(Value::Null).unwrap();
        stop.cancel();
        remote.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn prepared_restore_resource_locks_never_hold_priority_intent_gate() {
    let fixture = Fixture::new().await;
    let preferences = AudioPreferences::new(&json!({}));
    let intents = Arc::new(CriticalIntents::default());
    {
        let controls = preferences.controls.settings.lock().unwrap();
        assert!(preferences
            .try_prepare_session("alice", fixture.hub.clone(), None)
            .unwrap()
            .is_none());
        assert!(preferences.settings().is_some());
        drop(controls);
    }
    let prepared = preferences
        .try_prepare_session("alice", fixture.hub.clone(), None)
        .unwrap()
        .unwrap();
    assert!(preferences
        .try_prepare_session("bob", fixture.hub.clone(), None)
        .unwrap()
        .is_none());
    let (sent, received) = std::sync::mpsc::channel();
    let requesting = intents.clone();
    let task = std::thread::spawn(move || {
        requesting.request(CriticalAction::Logout);
        sent.send(()).unwrap();
    });
    let prompt = received.recv_timeout(Duration::from_millis(1500));
    drop(prepared);
    task.join().unwrap();
    prompt.unwrap();
    assert!(!preferences.edit("alice", "outputVolume", json!(175)));
}

#[tokio::test]
async fn prepared_logout_matches_legacy_without_losing_discarded_saves_or_touching_next_owner() {
    timeout(Duration::from_secs(8), async {
        let mut fixture = Fixture::new().await;
        let preferences = AudioPreferences::new(&json!({"language":"en"}));
        let legacy = AudioPreferences::new(&json!({"language":"en"}));
        for prefs in [&preferences, &legacy] {
            assert!(prefs.begin_session("alice", fixture.hub.clone(), None));
            assert!(prefs.edit("alice", "inputVolume", json!(175)));
        }
        let stop = CancellationToken::new();
        let failures = Arc::new(AtomicUsize::new(0));
        let observed = failures.clone();
        let remote = tokio::spawn(preferences.clone().run_remote(
            stop.clone(),
            Arc::new(move || {
                observed.fetch_add(1, Ordering::AcqRel);
            }),
        ));
        let pending = fixture.request().await;
        for revoked in [false, true] {
            let intents = CriticalIntents::default();
            let ticket = intents.ticket();
            let mut logout = preferences.try_prepare_logout().unwrap().unwrap();
            assert!(preferences.try_prepare_logout().unwrap().is_none());
            if revoked {
                intents.request(CriticalAction::Logout);
                assert!(intents
                    .commit(ticket, IntentScope::Session, || logout.apply())
                    .is_none());
            }
            drop(logout);
            assert!(preferences.edit("alice", "inputVolume", json!(175)));
        }
        pending.reply.send(Value::Null).unwrap();
        assert!(timeout(Duration::from_millis(650), fixture.requests.recv())
            .await
            .is_err());
        let controls_revision = preferences
            .controls
            .settings_revision
            .load(Ordering::Acquire);
        legacy.logout();
        {
            let mut logout = preferences.try_prepare_logout().unwrap().unwrap();
            logout.apply();
            logout.apply();
        }
        assert_eq!(preferences.settings(), legacy.settings());
        assert_eq!(
            preferences
                .controls
                .settings_revision
                .load(Ordering::Acquire),
            controls_revision
        );
        assert!(!preferences.edit("alice", "outputVolume", json!(140)));
        assert!(preferences.begin_session(
            "bob",
            fixture.hub.clone(),
            Some(&json!({"inputVolume":31}))
        ));
        assert!(preferences.edit("bob", "outputVolume", json!(140)));
        let latest = fixture.request().await;
        assert_eq!(latest.settings["inputVolume"], json!(31.0));
        assert_eq!(latest.settings["outputVolume"], json!(140.0));
        latest.reply.send(Value::Null).unwrap();
        stop.cancel();
        remote.await.unwrap();
        assert_eq!(failures.load(Ordering::Acquire), 0);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn untouched_profile_is_not_rewritten_and_storage_failure_is_explicit() {
    let temporary = tempfile::tempdir().unwrap();
    let profile = Arc::new(Profile::open(temporary.path()).unwrap());
    profile
        .write_settings(&json!({"language":"en","originalFormatting":true}))
        .unwrap();
    let original = std::fs::read(temporary.path().join("settings.json")).unwrap();
    let preferences = AudioPreferences::new(&profile.read_settings().unwrap());
    let stop = CancellationToken::new();
    stop.cancel();
    preferences
        .clone()
        .run_local(Some(profile.clone()), stop, no_failure())
        .await;
    assert_eq!(
        std::fs::read(temporary.path().join("settings.json")).unwrap(),
        original
    );
    assert!(preferences.language("ru"));
    std::fs::remove_file(temporary.path().join("settings.json")).unwrap();
    std::fs::create_dir(temporary.path().join("settings.json")).unwrap();
    let failures = Arc::new(AtomicUsize::new(0));
    let observed = failures.clone();
    let stop = CancellationToken::new();
    stop.cancel();
    preferences
        .run_local(
            Some(profile),
            stop,
            Arc::new(move || {
                observed.fetch_add(1, Ordering::AcqRel);
            }),
        )
        .await;
    assert_eq!(failures.load(Ordering::Acquire), 1);
    assert_eq!(
        std::fs::read_dir(temporary.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count(),
        0
    );
}

#[tokio::test]
async fn checked_final_save_returns_failure_instead_of_only_emitting_a_toast() {
    let temporary = tempfile::tempdir().unwrap();
    let profile = Arc::new(Profile::open(temporary.path()).unwrap());
    let preferences = AudioPreferences::new(&json!({}));
    assert!(preferences.language("en"));
    std::fs::create_dir(temporary.path().join("settings.json")).unwrap();
    let stop = CancellationToken::new();
    stop.cancel();
    let failures = Arc::new(AtomicUsize::new(0));
    let observed = failures.clone();
    assert!(preferences
        .run_local_checked(
            Some(profile),
            stop,
            Arc::new(move || {
                observed.fetch_add(1, Ordering::AcqRel);
            }),
        )
        .await
        .is_err());
    assert_eq!(failures.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn checked_final_save_observes_latest_original_settings_and_preserves_untouched_bytes() {
    let temporary = tempfile::tempdir().unwrap();
    let profile = Arc::new(Profile::open(temporary.path()).unwrap());
    profile
        .write_settings(&json!({"language":"ru","untouched":true}))
        .unwrap();
    let original = std::fs::read(temporary.path().join("settings.json")).unwrap();
    let preferences = AudioPreferences::new(&profile.read_settings().unwrap());
    let stop = CancellationToken::new();
    stop.cancel();
    preferences
        .clone()
        .run_local_checked(Some(profile.clone()), stop, no_failure())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(temporary.path().join("settings.json")).unwrap(),
        original
    );
    assert!(preferences.language("en"));
    let stop = CancellationToken::new();
    stop.cancel();
    preferences
        .run_local_checked(Some(profile.clone()), stop, no_failure())
        .await
        .unwrap();
    assert_eq!(profile.read_settings().unwrap()["language"], json!("en"));
}
