use crate::protocol::{self, Decoder, MAX_RECORD_BYTES};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Instant};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest, protocol::WebSocketConfig, Message,
};
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HubError {
    #[error("connection closed")]
    Closed,
    #[error("request timed out")]
    Timeout,
    #[error("connection unavailable")]
    Network,
    #[error("invalid server response")]
    Protocol,
    #[error("client queue is full")]
    Busy,
    #[error("server refused the operation")]
    Server(String),
    #[error("invalid server endpoint")]
    Endpoint,
}

#[derive(Clone)]
pub struct HubEvent {
    pub target: String,
    pub arguments: Vec<Value>,
}

struct Request {
    target: String,
    arguments: Vec<Value>,
    deadline: Instant,
    reply: oneshot::Sender<Result<Value, HubError>>,
    notification: bool,
}

struct Inner {
    requests: mpsc::Sender<Request>,
    stop: CancellationToken,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[derive(Clone)]
pub struct HubConnection(Arc<Inner>);

impl HubConnection {
    pub async fn connect(
        endpoint: &str,
        attestation: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<HubEvent>), HubError> {
        timeout(
            Duration::from_secs(35),
            Self::connect_inner(endpoint, attestation),
        )
        .await
        .map_err(|_| HubError::Timeout)?
    }

    async fn connect_inner(
        endpoint: &str,
        attestation: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<HubEvent>), HubError> {
        let mut url = validate_endpoint(endpoint)?;
        let mut negotiate = url.clone();
        negotiate.set_path(&format!("{}/negotiate", url.path().trim_end_matches('/')));
        negotiate
            .query_pairs_mut()
            .append_pair("negotiateVersion", "1");
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| HubError::Network)?;
        let mut request = client.post(negotiate).body("");
        if let Some(token) = attestation {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.map_err(|_| HubError::Network)?;
        if !response.status().is_success() {
            return Err(HubError::Network);
        }
        let mut bytes = Vec::new();
        let mut response = response;
        while let Some(chunk) = response.chunk().await.map_err(|_| HubError::Network)? {
            if bytes.len() + chunk.len() > 65536 {
                return Err(HubError::Protocol);
            }
            bytes.extend_from_slice(&chunk);
        }
        let negotiation: Value = serde_json::from_slice(&bytes).map_err(|_| HubError::Protocol)?;
        if negotiation.get("url").is_some() || negotiation.get("error").is_some() {
            return Err(HubError::Protocol);
        }
        let token = negotiation["connectionToken"]
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= 4096)
            .ok_or(HubError::Protocol)?;
        let websocket_available =
            negotiation["availableTransports"]
                .as_array()
                .is_some_and(|transports| {
                    transports.iter().any(|transport| {
                        transport["transport"] == "WebSockets"
                            && transport["transferFormats"]
                                .as_array()
                                .is_some_and(|formats| {
                                    formats.iter().any(|format| format == "Text")
                                })
                    })
                });
        if !websocket_available {
            return Err(HubError::Protocol);
        }
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme).map_err(|_| HubError::Endpoint)?;
        url.query_pairs_mut().append_pair("id", token);
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| HubError::Endpoint)?;
        if let Some(token) = attestation {
            request.headers_mut().insert(
                "Authorization",
                format!("Bearer {token}")
                    .parse()
                    .map_err(|_| HubError::Endpoint)?,
            );
        }
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_RECORD_BYTES + 1))
            .max_frame_size(Some(MAX_RECORD_BYTES + 1));
        let (mut socket, _) =
            tokio_tungstenite::connect_async_with_config(request, Some(config), false)
                .await
                .map_err(|_| HubError::Network)?;
        socket
            .send(Message::text("{\"protocol\":\"json\",\"version\":1}\u{1e}"))
            .await
            .map_err(|_| HubError::Network)?;
        let mut decoder = Decoder::default();
        let mut initial = Vec::new();
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let mut records = decoder
                        .push(text.as_bytes())
                        .map_err(|_| HubError::Protocol)?;
                    if records.is_empty() {
                        continue;
                    }
                    let handshake = records.remove(0);
                    if let Some(error) = handshake.get("error").and_then(Value::as_str) {
                        return Err(HubError::Server(error.to_owned()));
                    }
                    if handshake.get("type").is_some() {
                        return Err(HubError::Protocol);
                    }
                    initial.append(&mut records);
                    break;
                }
                Some(Ok(Message::Ping(bytes))) => {
                    socket
                        .send(Message::Pong(bytes))
                        .await
                        .map_err(|_| HubError::Network)?;
                }
                _ => return Err(HubError::Closed),
            }
        }
        let (requests_tx, mut requests_rx) = mpsc::channel::<Request>(64);
        let (events_tx, events_rx) = mpsc::channel(128);
        let stop = CancellationToken::new();
        let worker_stop = stop.clone();
        tokio::spawn(async move {
            let mut pending = BTreeMap::<String, Request>::new();
            let mut next_id = 0_u64;
            let mut clock = tokio::time::interval(Duration::from_millis(100));
            clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut last_receive = Instant::now();
            let mut last_send = Instant::now();
            let mut failure = HubError::Closed;
            for record in initial {
                if let Err(error) = dispatch(record, &mut pending, &events_tx) {
                    failure = error;
                    worker_stop.cancel();
                    break;
                }
            }
            loop {
                tokio::select! {
                    _ = worker_stop.cancelled() => break,
                    _ = events_tx.closed() => break,
                    request = requests_rx.recv() => {
                        let Some(mut request) = request else { break; };
                        if request.reply.is_closed() { continue; }
                        if pending.len() >= 64 { let _ = request.reply.send(Err(HubError::Busy)); continue; }
                        if request.deadline <= Instant::now() { let _ = request.reply.send(Err(HubError::Timeout)); continue; }
                        let Some(id) = next_id.checked_add(1) else { break; };
                        next_id = id;
                        let encoded = match if request.notification { protocol::encode(&serde_json::json!({"type":1,"target":request.target,"arguments":std::mem::take(&mut request.arguments)})) } else { protocol::invocation(id, &request.target, std::mem::take(&mut request.arguments)) } {
                            Ok(encoded) => encoded,
                            Err(_) => { let _ = request.reply.send(Err(HubError::Protocol)); continue; }
                        };
                        if !matches!(timeout(Duration::from_secs(5), socket.send(Message::text(encoded))).await, Ok(Ok(()))) {
                            let _ = request.reply.send(Err(HubError::Network));
                            failure = HubError::Network;
                            break;
                        }
                        last_send = Instant::now();
                        if request.notification { let _ = request.reply.send(Ok(Value::Null)); }
                        else { pending.insert(id.to_string(), request); }
                    }
                    message = socket.next() => {
                        last_receive = Instant::now();
                        match message {
                            Some(Ok(Message::Text(text))) => {
                                let records = match decoder.push(text.as_bytes()) { Ok(records) => records, Err(_) => { failure = HubError::Protocol; break; } };
                                let mut failed = false;
                                for record in records {
                                    if let Err(error) = dispatch(record, &mut pending, &events_tx) { failure = error; failed = true; break; }
                                }
                                if failed { break; }
                            }
                            Some(Ok(Message::Ping(bytes))) => {
                                if !matches!(timeout(Duration::from_secs(5), socket.send(Message::Pong(bytes))).await, Ok(Ok(()))) { break; }
                            }
                            Some(Ok(Message::Pong(_))) => {}
                            _ => break,
                        }
                    }
                    _ = clock.tick() => {
                        let now = Instant::now();
                        let expired: Vec<String> = pending.iter().filter(|(_, request)| request.deadline <= now || request.reply.is_closed()).map(|(id, _)| id.clone()).collect();
                        for id in expired { if let Some(request) = pending.remove(&id) { let _ = request.reply.send(Err(HubError::Timeout)); } }
                        if now.duration_since(last_receive) > Duration::from_secs(30) { failure = HubError::Timeout; break; }
                        if now.duration_since(last_send) >= Duration::from_secs(8) {
                            if !matches!(timeout(Duration::from_secs(5), socket.send(Message::text("{\"type\":6}\u{1e}"))).await, Ok(Ok(()))) { break; }
                            last_send = now;
                        }
                    }
                }
            }
            worker_stop.cancel();
            requests_rx.close();
            for (_, request) in pending {
                let _ = request.reply.send(Err(failure.clone()));
            }
            while let Ok(request) = requests_rx.try_recv() {
                let _ = request.reply.send(Err(failure.clone()));
            }
            let _ = timeout(Duration::from_secs(1), socket.close(None)).await;
        });
        Ok((
            Self(Arc::new(Inner {
                requests: requests_tx,
                stop,
            })),
            events_rx,
        ))
    }

    pub async fn invoke(
        &self,
        target: &str,
        arguments: Vec<Value>,
        duration: Duration,
    ) -> Result<Value, HubError> {
        if self.0.stop.is_cancelled() {
            return Err(HubError::Closed);
        }
        let (reply, response) = oneshot::channel();
        self.0
            .requests
            .try_send(Request {
                target: target.to_owned(),
                arguments,
                deadline: Instant::now() + duration,
                reply,
                notification: false,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => HubError::Busy,
                mpsc::error::TrySendError::Closed(_) => HubError::Closed,
            })?;
        timeout(duration, response)
            .await
            .map_err(|_| HubError::Timeout)?
            .map_err(|_| HubError::Closed)?
    }

    pub fn is_connected(&self) -> bool {
        !self.0.stop.is_cancelled()
    }
    pub fn close(&self) {
        self.0.stop.cancel();
    }

    pub async fn send(&self, target: &str, arguments: Vec<Value>) -> Result<(), HubError> {
        if self.0.stop.is_cancelled() {
            return Err(HubError::Closed);
        }
        let duration = Duration::from_secs(5);
        let (reply, response) = oneshot::channel();
        self.0
            .requests
            .try_send(Request {
                target: target.to_owned(),
                arguments,
                deadline: Instant::now() + duration,
                reply,
                notification: true,
            })
            .map_err(|_| HubError::Busy)?;
        timeout(duration, response)
            .await
            .map_err(|_| HubError::Timeout)?
            .map_err(|_| HubError::Closed)??;
        Ok(())
    }
}

pub fn validate_endpoint(endpoint: &str) -> Result<Url, HubError> {
    let url = Url::parse(endpoint).map_err(|_| HubError::Endpoint)?;
    let loopback = url.host().is_some_and(|host| match host {
        url::Host::Domain(domain) => domain == "localhost",
        url::Host::Ipv4(address) => address.is_loopback(),
        url::Host::Ipv6(address) => address.is_loopback(),
    });
    if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(HubError::Endpoint);
    }
    Ok(url)
}

fn dispatch(
    record: Value,
    pending: &mut BTreeMap<String, Request>,
    events: &mpsc::Sender<HubEvent>,
) -> Result<(), HubError> {
    match record["type"].as_u64() {
        Some(1) => {
            let target = record["target"].as_str().ok_or(HubError::Protocol)?;
            let arguments = record["arguments"].as_array().ok_or(HubError::Protocol)?;
            events
                .try_send(HubEvent {
                    target: target.to_owned(),
                    arguments: arguments.clone(),
                })
                .map_err(|_| HubError::Busy)?;
        }
        Some(3) => {
            let id = record["invocationId"].as_str().ok_or(HubError::Protocol)?;
            if let Some(request) = pending.remove(id) {
                let result = match record.get("error") {
                    Some(Value::String(error)) => Err(HubError::Server(error.clone())),
                    Some(_) => Err(HubError::Protocol),
                    None => Ok(record.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = request.reply.send(result);
            }
        }
        Some(6) => {}
        Some(7) => {
            return Err(record
                .get("error")
                .and_then(Value::as_str)
                .map(|error| HubError::Server(error.to_owned()))
                .unwrap_or(HubError::Closed))
        }
        _ => return Err(HubError::Protocol),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoints_preserve_tls_and_do_not_accept_embedded_credentials() {
        assert!(validate_endpoint("http://127.0.0.1:9000/zabor_v3").is_ok());
        assert!(validate_endpoint("http://[::1]:9000/zabor_v3").is_ok());
        assert!(validate_endpoint("https://example.invalid/zabor_v3").is_ok());
        for endpoint in [
            "http://example.invalid/hub",
            "https://name:password@example.invalid/hub",
            "https://example.invalid/hub?access_token=x",
            "https://example.invalid/hub#part",
        ] {
            assert_eq!(validate_endpoint(endpoint), Err(HubError::Endpoint));
        }
    }

    #[test]
    fn completion_for_an_expired_request_does_not_complete_a_new_one() {
        let (tx, _rx) = mpsc::channel(2);
        let (reply, mut response) = oneshot::channel();
        let mut pending = BTreeMap::new();
        pending.insert(
            "2".to_owned(),
            Request {
                target: "Login".into(),
                arguments: vec![],
                deadline: Instant::now() + Duration::from_secs(1),
                reply,
                notification: false,
            },
        );
        dispatch(
            json!({"type":3,"invocationId":"1","result":true}),
            &mut pending,
            &tx,
        )
        .unwrap();
        assert!(response.try_recv().is_err());
        dispatch(
            json!({"type":3,"invocationId":"2","result":false}),
            &mut pending,
            &tx,
        )
        .unwrap();
        assert_eq!(response.try_recv().unwrap(), Ok(json!(false)));
    }
}
