use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use zabor_native::auth::{AuthClient, AuthError, Credentials, LoginOutcome};
use zabor_native::call_recovery::{self, CallRecovery, RecoveryAction, ResumeResult};
use zabor_native::hub::{HubConnection, HubError};
use zabor_native::protocol::{encode, Decoder};
use zeroize::Zeroizing;

const SESSION: &str = "0123456789abcdef0123456789abcdef";

async fn fixture(legacy: bool) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/zabor_v3", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut http, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(http.read_u8().await.unwrap());
            assert!(header.len() < 8192);
        }
        let body = json!({"connectionToken":"call-fixture","negotiateVersion":1,"availableTransports":[{"transport":"WebSockets","transferFormats":["Text"]}]}).to_string();
        http.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        drop(http);
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap().into_text().unwrap(),
            "{\"protocol\":\"json\",\"version\":1}\u{1e}"
        );
        socket.send(Message::text("{}\u{1e}")).await.unwrap();
        let mut methods = Vec::new();
        let mut decoder = Decoder::default();
        let mut resumed = 0;
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            for request in decoder.push(text.as_bytes()).unwrap() {
                if request["type"] == 6 {
                    continue;
                }
                let method = request["target"].as_str().unwrap();
                methods.push(method.to_owned());
                let result = match method {
                    "CheckUserExists" => json!(true),
                    "Login" | "LoginAndResume" => {
                        assert_eq!(
                            request["arguments"].as_array().unwrap().len(),
                            if method == "Login" { 2 } else { 3 }
                        );
                        if method == "LoginAndResume" {
                            assert_eq!(request["arguments"][2], SESSION);
                        }
                        json!({"id":"owner","username":"fixture","displayName":"Fixture","isOnline":true})
                    }
                    "EnableCallResume" => {
                        assert_eq!(request["arguments"], json!([]));
                        Value::Null
                    }
                    "GetChannels" | "GetFriends" | "GetFriendRequests" | "GetChannelInvites" => {
                        json!([])
                    }
                    "GetAudioSettings" => json!("{}"),
                    "GetJokeOfTheDay" => json!("local"),
                    "GetIceServers" => json!({"iceServers":[]}),
                    "ResumeCall" => {
                        assert_eq!(request["arguments"], json!([SESSION]));
                        resumed += 1;
                        match resumed {
                            1 => json!({"state":"pending"}),
                            2 => {
                                json!({"state":"resumed","peer":{"id":"peer","username":"peer","displayName":"Peer","isOnline":true}})
                            }
                            _ => json!({"state":"ended"}),
                        }
                    }
                    "WaitResume" => continue,
                    _ => panic!("Unexpected local fixture method"),
                };
                let response = if legacy && method == "EnableCallResume" {
                    json!({"type":3,"invocationId":request["invocationId"],"error":"unknown capability"})
                } else {
                    json!({"type":3,"invocationId":request["invocationId"],"result":result})
                };
                socket
                    .send(Message::text(encode(&response).unwrap()))
                    .await
                    .unwrap();
            }
        }
        methods
    });
    (endpoint, task)
}

fn credentials() -> Credentials {
    Credentials {
        username: "fixture".into(),
        password: Zeroizing::new("fixture-only".into()),
    }
}

#[tokio::test]
async fn login_and_resume_use_existing_wire_contract_and_legacy_capability_is_optional() {
    timeout(Duration::from_secs(15), async {
        for legacy in [false, true] {
            let (endpoint, server) = fixture(legacy).await;
            let (hub, _events) = HubConnection::connect(&endpoint, None).await.unwrap();
            let auth = AuthClient::new(hub.clone());
            for session in [None, Some(SESSION)] {
                let outcome = auth
                    .login_with_resume(&credentials(), session)
                    .await
                    .unwrap();
                let LoginOutcome::SignedIn(data) = outcome else {
                    panic!("Login failed");
                };
                assert_eq!(data.user.id, "owner");
                assert!(data.channels.is_empty());
            }
            hub.close();
            let methods = server.await.unwrap();
            assert_eq!(
                methods.iter().filter(|method| *method == "Login").count(),
                1
            );
            assert_eq!(
                methods
                    .iter()
                    .filter(|method| *method == "LoginAndResume")
                    .count(),
                1
            );
            assert_eq!(
                methods
                    .iter()
                    .filter(|method| *method == "EnableCallResume")
                    .count(),
                2
            );
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn actual_hub_resume_pending_then_restored_then_ended_uses_one_session() {
    timeout(Duration::from_secs(10), async {
        let (endpoint, server) = fixture(false).await;
        let (hub, _events) = HubConnection::connect(&endpoint, None).await.unwrap();
        let mut recovery = CallRecovery::default();
        recovery.begin("peer");
        recovery.connected("peer");
        recovery.set_session(SESSION);
        let started = Instant::now();
        recovery.disconnected(started);
        let RecoveryAction::Resume {
            session,
            peer,
            timeout,
        } = recovery.poll(started + Duration::from_secs(1), true)
        else {
            panic!("Missing resume");
        };
        assert!(matches!(
            call_recovery::resume(&hub, &session, timeout)
                .await
                .unwrap(),
            ResumeResult::Pending
        ));
        recovery.retry(started + Duration::from_secs(1));
        let RecoveryAction::Resume {
            session, timeout, ..
        } = recovery.poll(started + Duration::from_secs(2), true)
        else {
            panic!("Missing retry");
        };
        let ResumeResult::Resumed { peer: user } = call_recovery::resume(&hub, &session, timeout)
            .await
            .unwrap()
        else {
            panic!("Missing peer");
        };
        assert_eq!(user.id, peer);
        assert!(recovery.restored(&session, &user.id, started + Duration::from_secs(2)));
        assert_eq!(recovery.next_deadline(), None);
        assert!(!recovery.accepts_end(None));
        assert!(recovery.accepts_end(Some(&session)));
        assert!(matches!(
            call_recovery::resume(&hub, &session, Duration::from_secs(5))
                .await
                .unwrap(),
            ResumeResult::Ended
        ));
        hub.close();
        assert_eq!(
            server.await.unwrap(),
            vec!["ResumeCall", "ResumeCall", "ResumeCall"]
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn invalid_sessions_and_zero_window_never_reach_network() {
    timeout(Duration::from_secs(10), async {
        let (endpoint, server) = fixture(false).await;
        let (hub, _events) = HubConnection::connect(&endpoint, None).await.unwrap();
        assert_eq!(
            AuthClient::new(hub.clone())
                .login_with_resume(&credentials(), Some("invalid"))
                .await
                .err(),
            Some(AuthError::Network)
        );
        assert!(matches!(
            call_recovery::resume(&hub, "invalid", Duration::from_secs(5)).await,
            Err(HubError::Protocol)
        ));
        assert!(matches!(
            call_recovery::resume(&hub, SESSION, Duration::ZERO).await,
            Err(HubError::Timeout)
        ));
        hub.close();
        assert!(server.await.unwrap().is_empty());
    })
    .await
    .unwrap();
}
