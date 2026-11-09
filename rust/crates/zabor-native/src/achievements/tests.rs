use super::*;
use crate::critical_intents::CriticalAction;
use crate::reference;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message;

fn translate(snapshot: &reference::Snapshot, language: &str, key: &str) -> String {
    let mut value = snapshot.catalog(if language == "en" { "en" } else { "ru" });
    for segment in key.split('.') {
        value = &value[segment];
    }
    value.as_str().unwrap_or(key).into()
}

fn hidden() -> Value {
    json!({"id":"local-hidden","icon":"⭐","maxValue":120,"statKey":"localCount","unit":"min","category":"social","titleRu":"локальное","descriptionRu":"локальный тест","titleEn":"local","descriptionEn":"local test"})
}

fn data() -> AchievementData {
    parse_result(&json!(json!({"stats":{"totalVoiceMinutes":119,"totalCalls":1},"unlockedIds":["first_friend","local-hidden"],"visitedChannelIds":["local-channel"],"totalHiddenCount":10,"unlockedHiddenDefs":[hidden()]}).to_string())).unwrap()
}

fn own_view(intents: &CriticalIntents) -> (AchievementsView, AchievementQuery) {
    let mut view = AchievementsView::default();
    let query = view
        .begin(
            intents,
            intents.ticket(),
            "local-owner",
            AchievementTarget::Own,
        )
        .unwrap();
    (view, query)
}

#[test]
fn envelope_defaults_and_null_fields_are_reference_defaults() {
    assert_eq!(
        parse_result(&json!("{}")).unwrap(),
        AchievementData::default()
    );
    assert_eq!(parse_result(&json!("{\"stats\":null,\"unlockedIds\":null,\"visitedChannelIds\":null,\"totalHiddenCount\":null,\"unlockedHiddenDefs\":null}")).unwrap(), AchievementData::default());
    let zero = parse_result(&json!("{\"TotalHiddenCount\":0}")).unwrap();
    assert_eq!(zero.total_hidden_count, 0);
}

#[test]
fn pascal_and_camel_envelopes_and_hidden_fields_match() {
    let camel = data();
    let mut pascal = json!({"Stats":camel.stats,"UnlockedIds":camel.unlocked_ids,"VisitedChannelIds":camel.visited_channel_ids,"TotalHiddenCount":10,"UnlockedHiddenDefs":[hidden()]});
    let definition = pascal["UnlockedHiddenDefs"][0].as_object_mut().unwrap();
    let old = std::mem::take(definition);
    for (key, value) in old {
        let name = format!("{}{}", key[..1].to_uppercase(), &key[1..]);
        definition.insert(name, value);
    }
    assert_eq!(parse_result(&json!(pascal.to_string())).unwrap(), camel);
}

#[test]
fn null_object_array_malformed_and_wrong_fields_do_not_fake_success() {
    for result in [Value::Null, json!(""), json!({}), json!(false)] {
        assert!(parse_result(&result).is_err());
    }
    for source in [
        "null",
        "[]",
        "[{}]",
        "1",
        "{",
        "{}{}",
        "{\"stats\":[]}",
        "{\"stats\":{\"n\":-1}}",
        "{\"stats\":{\"n\":1e999}}",
        "{\"unlockedIds\":[1]}",
        "{\"totalHiddenCount\":-1}",
        "{\"totalHiddenCount\":1.5}",
        "{\"unlockedHiddenDefs\":[{}]}",
    ] {
        assert!(parse_result(&json!(source)).is_err(), "{source}");
    }
}

#[test]
fn bytes_counts_text_numbers_and_nesting_are_bounded() {
    assert_eq!(
        parse_result(&json!(" ".repeat(MAX_PAYLOAD_BYTES + 1))),
        Err(AchievementError::Limit)
    );
    for envelope in [
        json!({"unlockedIds":vec!["id"; MAX_IDS + 1]}),
        json!({"visitedChannelIds":vec!["id"; MAX_IDS + 1]}),
        json!({"totalHiddenCount":MAX_HIDDEN + 1}),
        json!({"unlockedHiddenDefs":vec![hidden(); MAX_HIDDEN + 1]}),
        json!({"stats":(0..129).map(|index| (format!("n{index}"), 0)).collect::<BTreeMap<_, _>>()}),
        json!({"unlockedIds":["x".repeat(257)]}),
    ] {
        assert_eq!(
            parse_result(&json!(envelope.to_string())),
            Err(AchievementError::Limit)
        );
    }
    let mut definition = hidden();
    definition["titleEn"] = json!("x".repeat(4097));
    assert_eq!(
        parse_result(&json!(
            json!({"unlockedHiddenDefs":[definition]}).to_string()
        )),
        Err(AchievementError::Limit)
    );
    let nested = format!("{{\"ignored\":{}0{}}}", "[".repeat(130), "]".repeat(130));
    assert!(parse_result(&json!(nested)).is_err());
    assert!(parse_result(&json!("{\"stats\":{\"n\":9007199254740992}}")).is_err());
}

#[test]
fn duplicate_ids_deduplicate_in_original_order_but_conflicting_keys_fail() {
    let parsed = parse_result(&json!(json!({"unlockedIds":["b","a","b"],"visitedChannelIds":["c","c"],"unlockedHiddenDefs":[hidden(),hidden()]}).to_string())).unwrap();
    assert_eq!(parsed.unlocked_ids, ["b", "a"]);
    assert_eq!(parsed.visited_channel_ids, ["c"]);
    assert_eq!(parsed.unlocked_hidden_defs.len(), 1);
    for source in [
        "{\"stats\":{},\"Stats\":{}}",
        "{\"stats\":{\"n\":1,\"n\":2}}",
        "{\"unlockedIds\":[],\"unlockedIds\":[]}",
    ] {
        assert_eq!(parse_result(&json!(source)), Err(AchievementError::Invalid));
    }
    let mut conflict = hidden();
    conflict["titleEn"] = json!("different");
    assert!(parse_result(&json!(
        json!({"unlockedHiddenDefs":[hidden(),conflict]}).to_string()
    ))
    .is_err());
    let mut collision = hidden();
    collision["id"] = json!("first_friend");
    assert!(parse_result(&json!(json!({"unlockedHiddenDefs":[collision]}).to_string())).is_err());
}

#[test]
fn standard_definitions_match_original_ids_stats_categories_max_and_units() {
    let snapshot = reference::Snapshot::admit("achievements");
    let source = snapshot.text("src/renderer/src/achievements.ts");
    let definitions = &source[source.find("export const ACHIEVEMENTS:").unwrap()
        ..source
            .find("export interface HiddenAchievementPayload")
            .unwrap()];
    let blocks: Vec<_> = definitions.split("id: '").skip(1).collect();
    assert_eq!(blocks.len(), STANDARD.len());
    for (block, standard) in blocks.iter().zip(&STANDARD) {
        assert!(block.starts_with(&format!("{}'", standard.id)));
        assert!(block.contains(&format!("icon: '{}'", standard.icon)));
        assert!(block.contains(&format!("maxValue: {},", standard.max_value)));
        assert!(block.contains(&format!("statKey: '{}'", standard.stat_key)));
        let category = match standard.category {
            Category::Voice => "voice",
            Category::Calls => "calls",
            Category::Social => "social",
            Category::Hidden => "hidden",
        };
        assert!(block.contains(&format!("category: '{category}'")));
        assert_eq!(
            block.contains("unit: 'min'"),
            standard.unit == Some(Unit::Minutes)
        );
        for language in ["ru", "en"] {
            for field in ["title", "description"] {
                let key = format!("achievements.{}.{field}", standard.id);
                assert_ne!(translate(&snapshot, language, &key), key);
            }
        }
    }
}

#[test]
fn exact_progress_rounds_minutes_to_hours_not_fractional_bar() {
    for (value, maximum, expected, text) in [
        (59.0, 600.0, 0.0, "0 / 10 h"),
        (60.0, 600.0, 0.1, "1 / 10 h"),
        (119.0, 600.0, 0.1, "1 / 10 h"),
        (900.0, 600.0, 1.0, "10 / 10 h"),
        (1.0, 30.0, 0.0, "0 / 0 h"),
    ] {
        assert_eq!(
            progress(value, maximum, Some(Unit::Minutes), " h"),
            (expected, text.into())
        );
    }
    assert_eq!(progress(1.5, 3.0, None, " ч"), (0.5, "1.5 / 3".into()));
    assert_eq!(
        progress(119.0, 600.0, Some(Unit::Minutes), " ч"),
        (0.1, "1 / 10 ч".into())
    );
    assert_eq!(progress(0.0, 0.0, None, " h"), (0.0, "0 / 0".into()));
}

#[test]
fn own_peer_rows_localization_sort_and_unknown_hidden_differ() {
    let snapshot = reference::Snapshot::admit("catalogs");
    let intents = CriticalIntents::default();
    let (mut view, query) = own_view(&intents);
    assert!(view.data().is_none());
    assert!(view
        .rows("en", |key| translate(&snapshot, "en", key), None)
        .is_empty());
    assert!(view.accept(&intents, &query, Ok(data())));
    for language in ["ru", "en"] {
        let rows = view.rows(language, |key| translate(&snapshot, language, key), None);
        assert_eq!(rows.len(), 25);
        assert_eq!(rows[0].id, "local-hidden");
        assert_eq!(rows[0].category, Category::Hidden);
        assert!(rows[0].unlocked);
        assert_eq!(
            rows[0].title,
            if language == "en" {
                "local"
            } else {
                "локальное"
            }
        );
        assert_eq!(rows[1].id, "first_friend");
        assert_eq!(rows[1].progress, 1.0);
        assert_eq!(rows[2].id, "first_call");
        assert_eq!(rows.iter().filter(|row| row.unknown).count(), 9);
    }
    let peer = view
        .begin(
            &intents,
            intents.ticket(),
            "local-owner",
            AchievementTarget::Peer("local-peer".into()),
        )
        .unwrap();
    assert!(view.accept(&intents, &peer, Ok(data())));
    let rows = view.rows("en", |key| translate(&snapshot, "en", key), None);
    assert_eq!(rows.len(), 16);
    assert!(!rows.iter().any(|row| row.unknown));
    assert!(HiddenCatalog::empty("local-owner")
        .unwrap()
        .prepare(&view)
        .unwrap()
        .is_none());
}

#[test]
fn pending_duplicate_selection_close_duplicate_reply_logout_and_leave_guards() {
    let intents = CriticalIntents::default();
    let (mut view, old) = own_view(&intents);
    assert_eq!(
        view.begin(
            &intents,
            intents.ticket(),
            "local-owner",
            AchievementTarget::Own
        ),
        Err(AchievementError::Busy)
    );
    let new = view
        .begin(
            &intents,
            intents.ticket(),
            "local-owner",
            AchievementTarget::Peer("local-peer".into()),
        )
        .unwrap();
    assert!(!view.accept(&intents, &old, Ok(data())));
    intents.request(CriticalAction::Leave);
    assert!(view.accept(&intents, &new, Ok(data())));
    assert!(!view.accept(&intents, &new, Ok(data())));
    let closed = view
        .begin(
            &intents,
            intents.ticket(),
            "local-owner",
            AchievementTarget::Own,
        )
        .unwrap();
    view.close();
    assert!(!view.accept(&intents, &closed, Ok(data())));
    let stale = view
        .begin(
            &intents,
            intents.ticket(),
            "local-owner",
            AchievementTarget::Own,
        )
        .unwrap();
    intents.request(CriticalAction::Logout);
    assert!(!view.accept(&intents, &stale, Ok(data())));
    assert_eq!(
        view.begin(
            &intents,
            stale.ticket(),
            "local-owner",
            AchievementTarget::Own
        ),
        Err(AchievementError::Cancelled)
    );
}

#[test]
fn error_acceptance_is_explicit_empty_not_achievement_grant() {
    let snapshot = reference::Snapshot::admit("catalogs");
    let intents = CriticalIntents::default();
    let (mut view, query) = own_view(&intents);
    assert!(view.accept(&intents, &query, Err(AchievementError::Missing)));
    assert_eq!(view.data(), Some(&AchievementData::default()));
    assert_eq!(view.error(), Some(&AchievementError::Missing));
    assert!(HiddenCatalog::empty("local-owner")
        .unwrap()
        .prepare(&view)
        .unwrap()
        .is_none());
    assert!(!view
        .rows("en", |key| translate(&snapshot, "en", key), None)
        .iter()
        .any(|row| row.unlocked));
}

#[test]
fn own_durable_primary_boundary_failure_stale_logout_and_catalog_isolation() {
    let snapshot = reference::Snapshot::admit("catalogs");
    let intents = CriticalIntents::default();
    let (mut view, query) = own_view(&intents);
    view.accept(&intents, &query, Ok(data()));
    let catalog = HiddenCatalog::empty("local-owner").unwrap();
    let plan = catalog.prepare(&view).unwrap().unwrap();
    let json = plan.prepared_json().to_owned();
    assert_eq!(plan.owner(), "local-owner");
    assert!(plan
        .commit_if_current(&intents, &view, |_| Err::<(), _>("local failure"))
        .unwrap()
        .is_err());
    let plan = catalog.prepare(&view).unwrap().unwrap();
    let committed = plan
        .commit_if_current(&intents, &view, |_| Ok::<(), ()>(()))
        .unwrap()
        .unwrap();
    assert_eq!(
        HiddenCatalog::from_durable("local-owner", &json).unwrap(),
        committed
    );
    let mut missing = data();
    missing.unlocked_hidden_defs.clear();
    let refreshed = view
        .begin(
            &intents,
            intents.ticket(),
            "local-owner",
            AchievementTarget::Own,
        )
        .unwrap();
    view.accept(&intents, &refreshed, Ok(missing.clone()));
    assert_eq!(
        view.rows(
            "en",
            |key| translate(&snapshot, "en", key),
            Some(&committed)
        )[0]
        .id,
        "local-hidden"
    );
    let wrong_owner = HiddenCatalog::from_durable("other-owner", &json).unwrap();
    assert_eq!(
        view.rows(
            "en",
            |key| translate(&snapshot, "en", key),
            Some(&wrong_owner)
        )
        .len(),
        25
    );
    assert!(wrong_owner.prepare(&view).unwrap().is_none());
    let plan = committed.prepare(&view).unwrap().unwrap();
    let peer = view
        .begin(
            &intents,
            intents.ticket(),
            "local-owner",
            AchievementTarget::Peer("local-peer".into()),
        )
        .unwrap();
    assert!(plan
        .commit_if_current::<()>(&intents, &view, |_| panic!("stale write"))
        .is_none());
    view.accept(&intents, &peer, Ok(missing));
    assert_eq!(
        view.rows(
            "en",
            |key| translate(&snapshot, "en", key),
            Some(&committed)
        )
        .len(),
        15
    );
    let own = view
        .begin(
            &intents,
            intents.ticket(),
            "local-owner",
            AchievementTarget::Own,
        )
        .unwrap();
    view.accept(&intents, &own, Ok(data()));
    let plan = committed.prepare(&view).unwrap().unwrap();
    intents.request(CriticalAction::Logout);
    assert!(plan
        .commit_if_current::<()>(&intents, &view, |_| panic!("logout write"))
        .is_none());
}

#[test]
fn durable_map_malformed_duplicate_key_and_mismatched_id_fail() {
    for source in ["null", "[]", "{", "{\"wrong\":{}}"] {
        assert!(HiddenCatalog::from_durable("local-owner", source).is_err());
    }
    let definition = hidden().to_string();
    let duplicate = format!("{{\"local-hidden\":{definition},\"local-hidden\":{definition}}}");
    assert!(HiddenCatalog::from_durable("local-owner", &duplicate).is_err());
    let wrong = json!({"wrong":hidden()}).to_string();
    assert!(HiddenCatalog::from_durable("local-owner", &wrong).is_err());
}

async fn server(
    result: Result<Value, ()>,
    held: bool,
) -> (
    String,
    oneshot::Receiver<Value>,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/local", listener.local_addr().unwrap());
    let (request_tx, request_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut http, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(http.read_u8().await.unwrap());
            assert!(header.len() < 8192);
        }
        let body = json!({"connectionToken":"local-fixture","availableTransports":[{"transport":"WebSockets","transferFormats":["Text"]}]}).to_string();
        http.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        drop(http);
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap().into_text().unwrap(),
            "{\"protocol\":\"json\",\"version\":1}\u{1e}"
        );
        socket.send(Message::text("{}\u{1e}")).await.unwrap();
        let mut decoder = crate::protocol::Decoder::default();
        let request = loop {
            let message = socket.next().await.unwrap().unwrap();
            if let Message::Text(text) = message {
                if let Some(request) = decoder
                    .push(text.as_bytes())
                    .unwrap()
                    .into_iter()
                    .find(|request| request["type"] == 1)
                {
                    break request;
                }
            }
        };
        let _ = request_tx.send(request.clone());
        if held {
            let _ = release_rx.await;
        }
        let response = match result {
            Ok(result) => json!({"type":3,"invocationId":request["invocationId"],"result":result}),
            Err(()) => {
                json!({"type":3,"invocationId":request["invocationId"],"error":"local refusal"})
            }
        };
        let _ = socket
            .send(Message::text(crate::protocol::encode(&response).unwrap()))
            .await;
        while let Some(Ok(message)) = socket.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
            if let Message::Text(text) = message {
                for extra in decoder.push(text.as_bytes()).unwrap() {
                    assert_ne!(extra["type"], 1, "unexpected RPC: {extra}");
                }
            }
        }
    });
    (endpoint, request_rx, release_tx, task)
}

#[tokio::test]
async fn actual_hub_own_peer_exact_arguments_no_view_or_grant_rpc() {
    for target in [
        AchievementTarget::Own,
        AchievementTarget::Peer("local-peer".into()),
    ] {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (endpoint, request, _, server) = server(Ok(json!("{}")), false).await;
            let (hub, _events) = HubConnection::connect(&endpoint, None).await.unwrap();
            let intents = CriticalIntents::default();
            let mut view = AchievementsView::default();
            let query = view
                .begin(&intents, intents.ticket(), "local-owner", target.clone())
                .unwrap();
            let result = load(&hub, &intents, &CancellationToken::new(), &query)
                .await
                .unwrap();
            assert_eq!(result, AchievementData::default());
            let request = request.await.unwrap();
            match target {
                AchievementTarget::Own => {
                    assert_eq!(request["target"], "GetMyAchievements");
                    assert_eq!(request["arguments"], json!([]));
                }
                AchievementTarget::Peer(_) => {
                    assert_eq!(request["target"], "GetUserAchievements");
                    assert_eq!(request["arguments"], json!(["local-peer"]));
                }
            }
            hub.close();
            server.await.unwrap();
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn actual_hub_error_null_malformed_and_oversize() {
    for (result, expected) in [
        (
            Err(()),
            AchievementError::Hub(HubError::Server("local refusal".into())),
        ),
        (Ok(Value::Null), AchievementError::Missing),
        (Ok(json!("{")), AchievementError::Invalid),
        (
            Ok(json!("x".repeat(MAX_PAYLOAD_BYTES + 1))),
            AchievementError::Limit,
        ),
    ] {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (endpoint, _, _, server) = server(result, false).await;
            let (hub, _events) = HubConnection::connect(&endpoint, None).await.unwrap();
            let intents = CriticalIntents::default();
            let (_, query) = own_view(&intents);
            assert_eq!(
                load(&hub, &intents, &CancellationToken::new(), &query).await,
                Err(expected)
            );
            hub.close();
            server.await.unwrap();
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn held_response_survives_leave_but_rejects_new_selected_view() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let (endpoint, request, release, server) = server(Ok(json!("{}")), true).await;
        let (hub, _events) = HubConnection::connect(&endpoint, None).await.unwrap();
        let intents = CriticalIntents::default();
        let (mut view, query) = own_view(&intents);
        let stop = CancellationToken::new();
        let load = load(&hub, &intents, &stop, &query);
        tokio::pin!(load);
        tokio::select! { biased; _ = request => {}, _ = &mut load => panic!("not held") }
        intents.request(CriticalAction::Leave);
        let newer = view
            .begin(
                &intents,
                intents.ticket(),
                "local-owner",
                AchievementTarget::Peer("local-peer".into()),
            )
            .unwrap();
        release.send(()).unwrap();
        let result = load.await.unwrap();
        assert!(!view.accept(&intents, &query, Ok(result)));
        assert_eq!(view.query(), Some(&newer));
        hub.close();
        server.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn held_response_logout_shutdown_timeout_and_predispatch_cancel() {
    for mode in [0, 1, 2] {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (endpoint, request, release, server) = server(Ok(json!("{}")), true).await;
            let (hub, _events) = HubConnection::connect(&endpoint, None).await.unwrap();
            let intents = CriticalIntents::default();
            let (_, query) = own_view(&intents);
            let stop = CancellationToken::new();
            let load = load_timed(&hub, &intents, &stop, &query, Duration::from_millis(100));
            tokio::pin!(load);
            tokio::select! { biased; _ = request => {}, _ = &mut load => panic!("not held") }
            if mode == 0 {
                intents.request(CriticalAction::Logout);
            }
            if mode == 1 {
                stop.cancel();
            }
            let expected = if mode == 2 {
                AchievementError::Hub(HubError::Timeout)
            } else {
                AchievementError::Cancelled
            };
            assert_eq!(load.await, Err(expected));
            release.send(()).unwrap();
            let cancelled = CancellationToken::new();
            cancelled.cancel();
            assert_eq!(
                load_timed(&hub, &intents, &cancelled, &query, Duration::ZERO).await,
                Err(AchievementError::Cancelled)
            );
            hub.close();
            server.await.unwrap();
        })
        .await
        .unwrap();
    }
}
