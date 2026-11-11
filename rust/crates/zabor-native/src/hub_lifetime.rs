use super::{HubConnection, HubError, HubEvent};
use futures_util::FutureExt;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

const MAX_JOBS: usize = 256;

#[derive(Default)]
struct State {
    sealed: bool,
    jobs: Vec<Arc<Job>>,
}

struct Job {
    join: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

#[derive(Default)]
pub struct HubLifetime {
    stop: CancellationToken,
    state: Mutex<State>,
    drain: tokio::sync::Mutex<()>,
    failed: AtomicBool,
}

impl HubLifetime {
    pub(super) fn socket_stop(&self) -> CancellationToken {
        self.stop.child_token()
    }

    pub(super) fn spawn(
        &self,
        work: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), HubError> {
        let mut state = self.state.lock().map_err(|_| HubError::Closed)?;
        state.jobs.retain(|job| {
            let Ok(mut join) = job.join.try_lock() else {
                return true;
            };
            if let Some(handle) = join.as_mut().filter(|handle| handle.is_finished()) {
                if let Some(result) = handle.now_or_never() {
                    if result.is_err() {
                        self.failed.store(true, Ordering::Release);
                    }
                    join.take();
                }
            }
            join.is_some()
        });
        if state.sealed || self.stop.is_cancelled() || state.jobs.len() >= MAX_JOBS {
            drop(state);
            drop(work);
            return Err(HubError::Closed);
        }
        let join = tokio::spawn(work);
        state.jobs.push(Arc::new(Job {
            join: tokio::sync::Mutex::new(Some(join)),
        }));
        Ok(())
    }

    pub async fn connect(
        self: &Arc<Self>,
        endpoint: &str,
        attestation: Option<&str>,
    ) -> Result<(HubConnection, mpsc::Receiver<HubEvent>), HubError> {
        let endpoint = endpoint.to_owned();
        let attestation = attestation.map(|value| Zeroizing::new(value.to_owned()));
        let lifetime = self.clone();
        let cancellation = self.stop.child_token();
        let waiter = cancellation.clone().drop_guard();
        let (sender, receiver) = oneshot::channel();
        self.spawn(async move {
            let result = tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err(HubError::Closed),
                result = tokio::time::timeout(
                    Duration::from_secs(35),
                    HubConnection::connect_inner(
                        &endpoint,
                        attestation.as_ref().map(|value| value.as_str()),
                        Some(&lifetime),
                    ),
                ) => result.unwrap_or(Err(HubError::Timeout)),
            };
            let _ = sender.send(result);
        })?;
        let result = receiver.await.map_err(|_| HubError::Closed)?;
        waiter.disarm();
        result
    }

    pub async fn finish(&self) -> Result<(), HubError> {
        let _drain = self.drain.lock().await;
        self.stop.cancel();
        let jobs = {
            let mut state = self.state.lock().map_err(|_| HubError::Closed)?;
            state.sealed = true;
            state.jobs.clone()
        };
        for job in &jobs {
            let mut join = job.join.lock().await;
            if let Some(handle) = join.as_mut() {
                if handle.await.is_err() {
                    self.failed.store(true, Ordering::Release);
                }
                join.take();
            }
        }
        self.state
            .lock()
            .map_err(|_| HubError::Closed)?
            .jobs
            .retain(|job| !jobs.iter().any(|finished| Arc::ptr_eq(job, finished)));
        if self.failed.load(Ordering::Acquire) {
            Err(HubError::Closed)
        } else {
            Ok(())
        }
    }
}

impl Drop for HubLifetime {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelled_drain_retains_original_job_and_seals_admission() {
        let lifetime = Arc::new(HubLifetime::default());
        let (started, entered) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let completed = Arc::new(AtomicBool::new(false));
        let observed = completed.clone();
        lifetime
            .spawn(async move {
                let _ = started.send(());
                let _ = released.await;
                observed.store(true, Ordering::Release);
            })
            .unwrap();
        entered.await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), lifetime.finish())
                .await
                .is_err()
        );
        assert!(!completed.load(Ordering::Acquire));
        assert!(lifetime.spawn(async {}).is_err());
        release.send(()).unwrap();
        lifetime.finish().await.unwrap();
        assert!(completed.load(Ordering::Acquire));
        assert!(lifetime.state.lock().unwrap().jobs.is_empty());
        lifetime.finish().await.unwrap();
    }

    #[tokio::test]
    async fn original_panic_is_sticky_after_pruning_and_repeated_drain() {
        let lifetime = HubLifetime::default();
        let (started, entered) = oneshot::channel();
        lifetime
            .spawn(async move {
                let _ = started.send(());
                panic!("synthetic original hub owner failure");
            })
            .unwrap();
        entered.await.unwrap();
        let job = lifetime.state.lock().unwrap().jobs[0].clone();
        {
            let join = job.join.lock().await;
            assert!(join.as_ref().unwrap().is_finished());
        }
        assert!(lifetime.spawn(async {}).is_ok());
        assert!(lifetime.finish().await.is_err());
        assert!(lifetime.finish().await.is_err());
    }

    #[tokio::test]
    async fn bounded_admission_keeps_all_pending_original_joins() {
        let lifetime = HubLifetime::default();
        let mut releases = Vec::new();
        for _ in 0..MAX_JOBS {
            let (release, released) = oneshot::channel::<()>();
            lifetime
                .spawn(async move {
                    let _ = released.await;
                })
                .unwrap();
            releases.push(release);
        }
        assert!(lifetime.spawn(async {}).is_err());
        assert_eq!(lifetime.state.lock().unwrap().jobs.len(), MAX_JOBS);
        drop(releases);
        lifetime.finish().await.unwrap();
        assert!(lifetime.state.lock().unwrap().jobs.is_empty());
    }

    #[tokio::test]
    async fn dropped_connect_waiter_retains_and_joins_actual_loopback_acquisition() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/hub", listener.local_addr().unwrap());
        let lifetime = Arc::new(HubLifetime::default());
        let (accepted, entered) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            let _ = accepted.send(());
            let mut byte = [0];
            assert_eq!(socket.read(&mut byte).await.unwrap(), 0);
            socket.shutdown().await.unwrap();
        });
        let owner = lifetime.clone();
        let connecting = tokio::spawn(async move { owner.connect(&endpoint, None).await });
        entered.await.unwrap();
        connecting.abort();
        assert!(matches!(connecting.await, Err(error) if error.is_cancelled()));
        assert!(!lifetime.state.lock().unwrap().jobs.is_empty());
        server.await.unwrap();
        lifetime.finish().await.unwrap();
        assert!(lifetime.state.lock().unwrap().jobs.is_empty());
    }

    async fn loopback_socket(
        lifetime: &Arc<HubLifetime>,
    ) -> (HubConnection, mpsc::Receiver<HubEvent>, JoinHandle<()>) {
        use futures_util::{SinkExt, StreamExt};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio_tungstenite::tungstenite::Message;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/hub", listener.local_addr().unwrap());
        let (accepted, entered) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut negotiate, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                negotiate.read_exact(&mut byte).await.unwrap();
                request.extend_from_slice(&byte);
                assert!(request.len() < 4096);
            }
            let body = br#"{"connectionToken":"synthetic","availableTransports":[{"transport":"WebSockets","transferFormats":["Text"]}]}"#;
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            negotiate.write_all(headers.as_bytes()).await.unwrap();
            negotiate.write_all(body).await.unwrap();
            negotiate.shutdown().await.unwrap();
            drop(negotiate);
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            assert!(matches!(socket.next().await, Some(Ok(Message::Text(_)))));
            socket.send(Message::text("{}\u{1e}")).await.unwrap();
            let _ = accepted.send(());
            loop {
                match socket.next().await {
                    Some(Ok(Message::Close(_))) => {
                        let _ = socket.flush().await;
                        break;
                    }
                    None => break,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => panic!("synthetic loopback socket error: {error}"),
                }
            }
        });
        let (connection, events) = lifetime.connect(&endpoint, None).await.unwrap();
        entered.await.unwrap();
        assert!(connection.is_connected());
        (connection, events, server)
    }

    #[tokio::test]
    async fn live_loopback_socket_is_retained_after_all_connection_handles_drop() {
        let lifetime = Arc::new(HubLifetime::default());
        let (connection, events, server) = loopback_socket(&lifetime).await;
        drop((connection, events));
        lifetime.finish().await.unwrap();
        server.await.unwrap();
        assert!(lifetime.state.lock().unwrap().jobs.is_empty());
    }

    #[tokio::test]
    async fn lifetime_shutdown_joins_actual_socket_while_external_handles_remain_alive() {
        let lifetime = Arc::new(HubLifetime::default());
        let (connection, mut events, server) = loopback_socket(&lifetime).await;
        lifetime.finish().await.unwrap();
        server.await.unwrap();
        assert!(!connection.is_connected());
        assert!(events.recv().await.is_none());
        assert!(lifetime.state.lock().unwrap().jobs.is_empty());
        drop(connection);
    }
}
