use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use zabor_native::auth::{AuthClient, AuthError, Credentials, LoginOutcome};
use zabor_native::hub::{HubConnection, HubError};
use zabor_native::protocol::{encode, Decoder};
use zeroize::Zeroizing;

async fn server(throttle: bool) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/zabor_v3", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut http, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(http.read_u8().await.unwrap());
            assert!(header.len() < 8192);
        }
        assert!(String::from_utf8(header)
            .unwrap()
            .starts_with("POST /zabor_v3/negotiate?negotiateVersion=1 "));
        let body = json!({"connectionToken":"local-fixture","negotiateVersion":1,"availableTransports":[{"transport":"WebSockets","transferFormats":["Text"]}]}).to_string();
        http.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        drop(http);
        let (tcp, _) = listener.accept().await.unwrap();
        let mut websocket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        assert_eq!(
            websocket
                .next()
                .await
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap(),
            "{\"protocol\":\"json\",\"version\":1}\u{1e}"
        );
        websocket
            .send(Message::text(
                "{}\u{1e}{\"type\":1,\"target\":\"SyncFullChannelState\",\"arguments\":[{}]}\u{1e}",
            ))
            .await
            .unwrap();
        let mut decoder = Decoder::default();
        let mut methods = Vec::new();
        while let Some(Ok(Message::Text(text))) = websocket.next().await {
            for request in decoder.push(text.as_bytes()).unwrap() {
                if request["type"] == 6 {
                    continue;
                }
                let target = request["target"].as_str().unwrap();
                methods.push(target.to_owned());
                if target == "WaitForever" {
                    continue;
                }
                let result = match target {
                    "CheckUserExists" => json!(true),
                    "Login" => {
                        assert_eq!(request["arguments"].as_array().unwrap().len(), 2);
                        json!({"id":"fixture-owner","username":"fixture","displayName":"Fixture","isOnline":true})
                    }
                    "GetChannels" => {
                        json!([{"id":"fixture-channel","name":"local","ownerId":"fixture-owner"}])
                    }
                    "GetFriends" | "GetFriendRequests" | "GetChannelInvites" => json!([]),
                    "GetAudioSettings" => json!("{\"inputVolume\":75}"),
                    "GetJokeOfTheDay" => json!("local fixture"),
                    "AcceptFriendRequest" | "DeclineFriendRequest" | "AcceptChannelInvite" => {
                        assert_eq!(request["arguments"], json!(["local-notification"]));
                        Value::Null
                    }
                    "DeclineChannelInvite" => json!(false),
                    _ => Value::Null,
                };
                let response = if throttle {
                    json!({"type":3,"invocationId":request["invocationId"],"error":"ZABOR_AUTH_THROTTLED:120"})
                } else {
                    json!({"type":3,"invocationId":request["invocationId"],"result":result})
                };
                websocket
                    .send(Message::text(encode(&response).unwrap()))
                    .await
                    .unwrap();
            }
        }
        methods
    });
    (endpoint, task)
}

#[tokio::test]
async fn actual_negotiate_websocket_login_bootstrap_events_and_timeout() {
    timeout(Duration::from_secs(10), async {
        let (endpoint, server) = server(false).await;
        let (hub, mut events) = HubConnection::connect(&endpoint, None).await.unwrap();
        assert_eq!(events.recv().await.unwrap().target, "SyncFullChannelState");
        let client = AuthClient::new(hub.clone());
        let credentials = Credentials {
            username: "fixture".into(),
            password: Zeroizing::new("local-test".into()),
        };
        let LoginOutcome::SignedIn(snapshot) = client.login(&credentials).await.unwrap() else {
            panic!("login did not complete")
        };
        assert_eq!(snapshot.user.id, "fixture-owner");
        assert_eq!(snapshot.channels.len(), 1);
        assert_eq!(snapshot.audio_settings.unwrap()["inputVolume"], 75);
        assert_eq!(
            hub.invoke("WaitForever", vec![], Duration::from_millis(50))
                .await,
            Err(HubError::Timeout)
        );
        hub.close();
        assert_eq!(
            hub.invoke("GetChannels", vec![], Duration::from_secs(1))
                .await,
            Err(HubError::Closed)
        );
        let methods = server.await.unwrap();
        assert_eq!(
            methods.iter().filter(|method| *method == "Login").count(),
            1
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn server_throttle_cannot_be_mistaken_for_a_missing_account() {
    timeout(Duration::from_secs(10), async {
        let (endpoint, server) = server(true).await;
        let (hub, _events) = HubConnection::connect(&endpoint, None).await.unwrap();
        let client = AuthClient::new(hub.clone());
        let credentials = Credentials {
            username: "fixture".into(),
            password: Zeroizing::new("local-test".into()),
        };
        assert!(matches!(
            client.login(&credentials).await,
            Err(AuthError::Throttled(2))
        ));
        hub.close();
        assert_eq!(server.await.unwrap(), vec!["CheckUserExists"]);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn notification_commands_keep_the_existing_null_success_and_false_rejection_contract() {
    use zabor_native::roster::{respond_notification, NotificationKind};
    timeout(Duration::from_secs(10), async {
        let (endpoint, server) = server(false).await;
        let (hub, _events) = HubConnection::connect(&endpoint, None).await.unwrap();
        for (kind, accept) in [
            (NotificationKind::Friend, true),
            (NotificationKind::Friend, false),
            (NotificationKind::Channel, true),
        ] {
            assert!(
                respond_notification(&hub, kind, "local-notification", accept)
                    .await
                    .unwrap()
            );
        }
        assert!(!respond_notification(
            &hub,
            NotificationKind::Channel,
            "local-notification",
            false
        )
        .await
        .unwrap());
        assert_eq!(
            respond_notification(&hub, NotificationKind::Friend, "", true).await,
            Err(HubError::Protocol)
        );
        hub.close();
        assert_eq!(
            server.await.unwrap(),
            [
                "AcceptFriendRequest",
                "DeclineFriendRequest",
                "AcceptChannelInvite",
                "DeclineChannelInvite"
            ]
        );
    })
    .await
    .unwrap();
}
