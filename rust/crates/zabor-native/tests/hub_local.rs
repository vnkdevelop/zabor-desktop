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
                    "UpdateProfile" => {
                        assert_eq!(request["arguments"].as_array().unwrap().len(), 4);
                        let user = json!({"id":"fixture-owner","username":"fixture","displayName":request["arguments"][0],"avatarBase64":request["arguments"][1],"avatarColor":request["arguments"][2],"aboutMe":request["arguments"][3],"isOnline":true});
                        websocket
                            .send(Message::text(
                                encode(
                                    &json!({"type":1,"target":"UserUpdated","arguments":[user]}),
                                )
                                .unwrap(),
                            ))
                            .await
                            .unwrap();
                        Value::Null
                    }
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

#[tokio::test]
async fn five_mib_packed_gif_profile_update_and_user_event_cross_actual_local_websocket_unchanged()
{
    use tokio_util::sync::CancellationToken;
    use zabor_native::auth::{ProfileUpdate, User};
    use zabor_native::avatar::{Crop, Source, MAX_FILE_BYTES};
    timeout(Duration::from_secs(15), async {
        let mut gif = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut gif);
            encoder
                .set_repeat(image::codecs::gif::Repeat::Infinite)
                .unwrap();
            for pixel in [[255, 0, 0, 255], [0, 0, 255, 255]] {
                encoder
                    .encode_frame(image::Frame::from_parts(
                        image::RgbaImage::from_pixel(8, 4, image::Rgba(pixel)),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(40, 1),
                    ))
                    .unwrap();
            }
        }
        assert_eq!(gif.pop(), Some(0x3b));
        gif.extend_from_slice(&[0x21, 0xfe]);
        while MAX_FILE_BYTES - gif.len() > 3 {
            let size = (MAX_FILE_BYTES - gif.len() - 3).min(255);
            gif.push(size as u8);
            gif.resize(gif.len() + size, 0);
        }
        gif.extend_from_slice(&[0, 0x3b]);
        assert!((MAX_FILE_BYTES - 1..=MAX_FILE_BYTES).contains(&gif.len()));
        let source = Source::from_bytes(&gif, &CancellationToken::new()).unwrap();
        let selected = source
            .select(
                Crop {
                    scale: 1.25,
                    x: -10.5,
                    y: 20.0,
                },
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(selected.avatar.frames.len(), 2);
        assert!(selected.wire.len() > 4 * 1024 * 1024);
        let (endpoint, server) = server(false).await;
        let (hub, mut events) = HubConnection::connect(&endpoint, None).await.unwrap();
        assert_eq!(events.recv().await.unwrap().target, "SyncFullChannelState");
        let user = User {
            id: "fixture-owner".into(),
            username: "fixture".into(),
            is_muted: true,
            ..Default::default()
        };
        let update = ProfileUpdate {
            owner: user.id.clone(),
            name: " имя ".into(),
            avatar: Some(selected.wire.clone()),
            color: selected.color,
            about: " о себе ".into(),
        };
        let result = AuthClient::new(hub.clone())
            .update_profile(&update, &user)
            .await
            .unwrap();
        assert!(result.is_muted);
        let event = events.recv().await.unwrap();
        assert_eq!(event.target, "UserUpdated");
        assert_eq!(event.arguments[0]["avatarBase64"], selected.wire);
        assert_eq!(event.arguments[0]["displayName"], "имя");
        assert_eq!(event.arguments[0]["aboutMe"], "о себе");
        hub.close();
        assert_eq!(server.await.unwrap(), ["UpdateProfile"]);
    })
    .await
    .unwrap();
}
