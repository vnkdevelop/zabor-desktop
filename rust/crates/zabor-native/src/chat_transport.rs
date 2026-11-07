use crate::chat::OutboundPacket;
use crate::hub::HubConnection;
use crate::media::{IceServerSettings, MediaEngine, MediaError, MediaEvent, PeerLifetime};
use libwebrtc::data_channel::{DataChannel, DataChannelInit, DataChannelState};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub enum ChatTransportEvent {
    Text(String, String),
    Binary(String, Vec<u8>),
    Ready(String),
    FilesReady(String),
    Unavailable(String),
    Ice(String, String),
}

struct TransportMessage {
    event: ChatTransportEvent,
    lifetime: PeerLifetime,
    active: Arc<AtomicBool>,
}

struct ChannelBinding {
    channel: DataChannel,
    active: Arc<AtomicBool>,
    lifetime: PeerLifetime,
    failed: Arc<AtomicBool>,
}

impl ChannelBinding {
    fn is_open(&self) -> bool {
        self.active.load(Ordering::Acquire)
            && self.lifetime.is_alive()
            && self.channel.state() == DataChannelState::Open
    }
}

impl Drop for ChannelBinding {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        self.channel.on_message(None);
        self.channel.on_state_change(None);
        self.channel.close();
    }
}

pub struct ChatTransport {
    engine: MediaEngine,
    media: mpsc::Receiver<MediaEvent>,
    events: mpsc::Receiver<TransportMessage>,
    sender: mpsc::Sender<TransportMessage>,
    control: BTreeMap<String, ChannelBinding>,
    files: BTreeMap<String, ChannelBinding>,
    connecting: BTreeMap<String, Instant>,
    stop: CancellationToken,
}

impl ChatTransport {
    pub fn new(
        owner: String,
        friends: impl IntoIterator<Item = String>,
        ice: Vec<IceServerSettings>,
        relay: bool,
    ) -> Result<Self, MediaError> {
        Self::with_stop(owner, friends, ice, relay, CancellationToken::new())
    }

    pub fn with_stop(
        owner: String,
        friends: impl IntoIterator<Item = String>,
        ice: Vec<IceServerSettings>,
        relay: bool,
        stop: CancellationToken,
    ) -> Result<Self, MediaError> {
        if stop.is_cancelled() {
            return Err(MediaError::Backend);
        }
        let (mut engine, media) = MediaEngine::new_data_only(owner);
        engine.authorize_peers(friends);
        engine.configure_ice(ice, relay)?;
        let (sender, events) = mpsc::channel(64);
        Ok(Self {
            engine,
            media,
            events,
            sender,
            control: BTreeMap::new(),
            files: BTreeMap::new(),
            connecting: BTreeMap::new(),
            stop,
        })
    }

    pub async fn connect(&mut self, hub: &HubConnection, peer: &str) -> Result<(), MediaError> {
        if self.stop.is_cancelled() {
            return Err(MediaError::Backend);
        }
        if self.control.get(peer).is_some_and(ChannelBinding::is_open)
            || self
                .connecting
                .get(peer)
                .is_some_and(|started| started.elapsed() < Duration::from_secs(10))
        {
            return Ok(());
        }
        self.disconnect_peer(peer);
        self.connecting.insert(peer.into(), Instant::now());
        let result = async {
            self.engine.disconnect_peer(peer);
            let control = self.engine.create_data_channel(
                peer,
                "zabor-chat-v1",
                DataChannelInit::default(),
            )?;
            let files = self.engine.create_data_channel(
                peer,
                "zabor-chat-files-v1",
                DataChannelInit::default(),
            )?;
            let lifetime = self.engine.peer_lifetime(peer).ok_or(MediaError::Backend)?;
            self.attach(peer.into(), control, lifetime.clone());
            self.attach(peer.into(), files, lifetime);
            let offer = self.engine.offer(peer, false).await?;
            hub.send(
                "SendWebRTCOffer",
                vec![json!(peer), json!(format!("zabor-chat:{offer}"))],
            )
            .await
            .map_err(|_| MediaError::Backend)
        }
        .await;
        if result.is_err() {
            self.connecting.remove(peer);
        }
        result
    }

    pub async fn handle_signal(
        &mut self,
        hub: &HubConnection,
        target: &str,
        peer: &str,
        payload: &str,
    ) -> Result<(), MediaError> {
        if self.stop.is_cancelled() {
            return Err(MediaError::Backend);
        }
        let payload = payload
            .strip_prefix("zabor-chat:")
            .ok_or(MediaError::InvalidSignal)?;
        match target {
            "ReceiveWebRTCOffer" => {
                if let Some(answer) = self.engine.receive_offer(peer, payload).await? {
                    hub.send(
                        "SendWebRTCAnswer",
                        vec![json!(peer), json!(format!("zabor-chat:{answer}"))],
                    )
                    .await
                    .map_err(|_| MediaError::Backend)?;
                }
            }
            "ReceiveWebRTCAnswer" => self.engine.receive_answer(peer, payload).await?,
            "ReceiveIceCandidate" => self.engine.receive_ice(peer, payload).await?,
            _ => return Err(MediaError::InvalidSignal),
        }
        Ok(())
    }

    pub fn send_control(&self, packet: &OutboundPacket) -> bool {
        !self.stop.is_cancelled()
            && self.control.get(&packet.peer).is_some_and(|binding| {
                binding.is_open()
                    && binding.channel.buffered_amount() < 1024 * 1024
                    && binding
                        .channel
                        .send(
                            packet
                                .direct
                                .as_deref()
                                .unwrap_or(&packet.payload)
                                .as_bytes(),
                            false,
                        )
                        .is_ok()
            })
    }

    pub fn file_channel(&self, peer: &str) -> Option<DataChannel> {
        if self.stop.is_cancelled() {
            return None;
        }
        self.files
            .get(peer)
            .filter(|binding| binding.is_open())
            .map(|binding| binding.channel.clone())
    }

    pub fn disconnect_peer(&mut self, peer: &str) {
        self.control.remove(peer);
        self.files.remove(peer);
        self.connecting.remove(peer);
        self.engine.disconnect_peer(peer);
    }

    pub fn set_friends(&mut self, friends: &[String]) {
        self.engine.authorize_peers(friends.iter().cloned());
        for channels in [&mut self.control, &mut self.files] {
            channels.retain(|peer, _| friends.contains(peer));
        }
        self.connecting.retain(|peer, _| friends.contains(peer));
    }

    pub async fn next_event(&mut self) -> Option<ChatTransportEvent> {
        loop {
            if self.stop.is_cancelled() {
                return None;
            }
            let failed = self
                .control
                .iter()
                .chain(self.files.iter())
                .find(|(_, binding)| binding.failed.load(Ordering::Acquire))
                .map(|(peer, _)| peer.clone());
            if let Some(peer) = failed {
                self.disconnect_peer(&peer);
                return Some(ChatTransportEvent::Unavailable(peer));
            }
            tokio::select! {
                _ = self.stop.cancelled() => return None,
                event = self.events.recv() => {
                    let event = event?;
                    if !self.stop.is_cancelled() && event.lifetime.is_alive() && event.active.load(Ordering::Acquire) {
                        if let ChatTransportEvent::Unavailable(peer) = &event.event { self.disconnect_peer(peer); }
                        return Some(event.event);
                    }
                },
                event = self.media.recv() => { let event = event?; if self.stop.is_cancelled() || !event.is_alive() { continue; } match event {
                    MediaEvent::Ice { peer, payload, .. } => return Some(ChatTransportEvent::Ice(peer, format!("zabor-chat:{payload}"))),
                    MediaEvent::DataChannel { peer, channel, lifetime } => self.attach(peer, channel, lifetime),
                    MediaEvent::Connection { peer, state, .. } => {
                        if matches!(state, libwebrtc::peer_connection::PeerConnectionState::Closed | libwebrtc::peer_connection::PeerConnectionState::Failed) { self.disconnect_peer(&peer); return Some(ChatTransportEvent::Unavailable(peer)); }
                    },
                    _ => {},
                } },
            }
        }
    }

    fn attach(&mut self, peer: String, channel: DataChannel, lifetime: PeerLifetime) {
        if self.stop.is_cancelled() {
            channel.close();
            return;
        }
        let file = channel.label() == "zabor-chat-files-v1";
        if !file && channel.label() != "zabor-chat-v1" {
            channel.close();
            return;
        }
        let sender = self.sender.clone();
        let id = peer.clone();
        let active = Arc::new(AtomicBool::new(true));
        let failed = Arc::new(AtomicBool::new(false));
        let callback_failed = failed.clone();
        let live = lifetime.clone();
        let callback_active = active.clone();
        let stop = self.stop.clone();
        channel.on_message(Some(Box::new(move |buffer| {
            if stop.is_cancelled()
                || !live.is_alive()
                || !callback_active.load(Ordering::Acquire)
                || buffer.data.len() > 256 * 1024
                || buffer.binary != file
            {
                return;
            }
            let event = if buffer.binary {
                ChatTransportEvent::Binary(id.clone(), buffer.data.to_vec())
            } else {
                let Ok(text) = std::str::from_utf8(buffer.data) else {
                    return;
                };
                ChatTransportEvent::Text(id.clone(), text.into())
            };
            if sender
                .try_send(TransportMessage {
                    event,
                    lifetime: live.clone(),
                    active: callback_active.clone(),
                })
                .is_err()
            {
                callback_failed.store(true, Ordering::Release);
            }
        })));
        let sender = self.sender.clone();
        let id = peer.clone();
        let live = lifetime.clone();
        let callback_active = active.clone();
        let callback_failed = failed.clone();
        let stop = self.stop.clone();
        channel.on_state_change(Some(Box::new(move |state| {
            if stop.is_cancelled() || !live.is_alive() || !callback_active.load(Ordering::Acquire) {
                return;
            }
            let event = match state {
                DataChannelState::Open => {
                    if file {
                        ChatTransportEvent::FilesReady(id.clone())
                    } else {
                        ChatTransportEvent::Ready(id.clone())
                    }
                }
                DataChannelState::Closed => {
                    callback_failed.store(true, Ordering::Release);
                    ChatTransportEvent::Unavailable(id.clone())
                }
                _ => return,
            };
            let _ = sender.try_send(TransportMessage {
                event,
                lifetime: live.clone(),
                active: callback_active.clone(),
            });
        })));
        if channel.state() == DataChannelState::Open {
            let _ = self.sender.try_send(TransportMessage {
                event: if file {
                    ChatTransportEvent::FilesReady(peer.clone())
                } else {
                    ChatTransportEvent::Ready(peer.clone())
                },
                lifetime: lifetime.clone(),
                active: active.clone(),
            });
        }
        let binding = ChannelBinding {
            channel,
            active,
            lifetime,
            failed,
        };
        if file {
            self.files.insert(peer, binding);
        } else {
            self.control.insert(peer, binding);
        }
    }
}

impl Drop for ChatTransport {
    fn drop(&mut self) {
        self.control.clear();
        self.files.clear();
        self.engine.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn revoked_owner_blocks_queued_transport_events_and_drop_closes_peer_lifetimes() {
        use crate::critical_intents::{CriticalAction, CriticalIntents};

        let intents = CriticalIntents::default();
        let stop = intents.session_token(intents.ticket());
        let mut transport =
            ChatTransport::with_stop("alice".into(), ["bob".into()], vec![], false, stop.clone())
                .unwrap();
        for label in ["zabor-chat-v1", "zabor-chat-files-v1"] {
            let channel = transport
                .engine
                .create_data_channel("bob", label, DataChannelInit::default())
                .unwrap();
            transport.attach(
                "bob".into(),
                channel,
                transport.engine.peer_lifetime("bob").unwrap(),
            );
        }
        let binding = transport.control.get("bob").unwrap();
        let lifetime = binding.lifetime.clone();
        let active = binding.active.clone();
        transport
            .sender
            .try_send(TransportMessage {
                event: ChatTransportEvent::Text("bob".into(), "old-owner".into()),
                lifetime: lifetime.clone(),
                active: active.clone(),
            })
            .unwrap_or_else(|_| panic!("Fixture queue failed"));
        intents.request(CriticalAction::Leave);
        assert!(!stop.is_cancelled());
        assert!(lifetime.is_alive());
        intents.request(CriticalAction::Logout);
        assert!(stop.is_cancelled());
        assert!(timeout(Duration::from_secs(2), transport.next_event())
            .await
            .unwrap()
            .is_none());
        assert!(!transport.send_control(&OutboundPacket {
            peer: "bob".into(),
            payload: "old-owner".into(),
            direct: None
        }));
        assert!(transport.file_channel("bob").is_none());
        drop(transport);
        assert!(!active.load(Ordering::Acquire));
        assert!(!lifetime.is_alive());
    }

    #[tokio::test]
    async fn direct_chat_delivers_and_discards_queued_packets_after_friend_revocation() {
        timeout(Duration::from_secs(15), async {
            let mut alice = ChatTransport::new("alice".into(), ["bob".into()], vec![], false).unwrap();
            let mut bob = ChatTransport::new("bob".into(), ["alice".into()], vec![], false).unwrap();
            let channel = alice.engine.create_data_channel("bob", "zabor-chat-v1", DataChannelInit::default()).unwrap();
            let lifetime = alice.engine.peer_lifetime("bob").unwrap();
            alice.attach("bob".into(), channel, lifetime);
            let offer = alice.engine.offer("bob", false).await.unwrap();
            let answer = bob.engine.receive_offer("alice", &offer).await.unwrap().unwrap();
            alice.engine.receive_answer("bob", &answer).await.unwrap();
            let mut ready = (false, false);
            while !ready.0 || !ready.1 {
                tokio::select! {
                    event = alice.next_event() => match event.unwrap() {
                        ChatTransportEvent::Ice(_, payload) => bob.engine.receive_ice("alice", payload.strip_prefix("zabor-chat:").unwrap()).await.unwrap(),
                        ChatTransportEvent::Ready(_) => ready.0 = true,
                        _ => {},
                    },
                    event = bob.next_event() => match event.unwrap() {
                        ChatTransportEvent::Ice(_, payload) => alice.engine.receive_ice("bob", payload.strip_prefix("zabor-chat:").unwrap()).await.unwrap(),
                        ChatTransportEvent::Ready(_) => ready.1 = true,
                        _ => {},
                    },
                }
            }
            let packet = OutboundPacket { peer: "bob".into(), payload: "fallback".into(), direct: Some("direct-fixture".into()) };
            assert!(alice.send_control(&packet));
            loop {
                if let ChatTransportEvent::Text(peer, text) = bob.next_event().await.unwrap() {
                    assert_eq!((peer.as_str(), text.as_str()), ("alice", "direct-fixture"));
                    break;
                }
            }
            assert!(alice.send_control(&packet));
            let queued = loop {
                let message = bob.events.recv().await.unwrap();
                if matches!(message.event, ChatTransportEvent::Text(_, _)) { break message; }
            };
            assert!(queued.lifetime.is_alive());
            assert!(queued.active.load(Ordering::Acquire));
            let lifetime = queued.lifetime.clone();
            let active = queued.active.clone();
            assert!(bob.sender.try_send(queued).is_ok());
            bob.set_friends(&[]);
            assert!(!lifetime.is_alive());
            assert!(!active.load(Ordering::Acquire));
            assert!(timeout(Duration::from_millis(50), bob.next_event()).await.is_err());
            assert!(!bob.send_control(&OutboundPacket { peer: "alice".into(), payload: "revoked".into(), direct: None }));
        }).await.unwrap();
    }

    #[tokio::test]
    async fn file_workflow_stages_signals_transfers_verifies_and_atomically_saves_over_sctp() {
        use crate::chat::ChatEngine;
        use crate::chat_crypto::ChatIdentity;
        use crate::chat_store::{ChatStore, RETENTION_MS};
        use crate::file_jobs::{FileEvent, FileJobs};

        timeout(Duration::from_secs(30), async {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("original file.bin");
            let target = root.path().join("saved file.bin");
            let bytes: Vec<u8> = (0..2 * 1024 * 1024).map(|index| (index % 251) as u8).collect();
            std::fs::write(&source, &bytes).unwrap();
            std::fs::write(&target, b"old destination").unwrap();
            let mut a = ChatEngine::new("alice".into(), ChatIdentity::generate(), ChatStore::open(&root.path().join("alice.sqlite")).unwrap());
            let mut b = ChatEngine::new("bob".into(), ChatIdentity::generate(), ChatStore::open(&root.path().join("bob.sqlite")).unwrap());
            a.set_friends(["bob".into()]); b.set_friends(["alice".into()]);
            a.set_file_root(&root.path().join("alice")).unwrap(); b.set_file_root(&root.path().join("bob")).unwrap();
            let mut jobs_a = FileJobs::new(&root.path().join("alice"));
            let mut jobs_b = FileJobs::new(&root.path().join("bob"));
            let mut alice = ChatTransport::new("alice".into(), ["bob".into()], vec![], false).unwrap();
            let mut bob = ChatTransport::new("bob".into(), ["alice".into()], vec![], false).unwrap();
            for label in ["zabor-chat-v1", "zabor-chat-files-v1"] {
                let channel = alice.engine.create_data_channel("bob", label, DataChannelInit::default()).unwrap();
                alice.attach("bob".into(), channel, alice.engine.peer_lifetime("bob").unwrap());
            }
            let offer = alice.engine.offer("bob", false).await.unwrap();
            let answer = bob.engine.receive_offer("alice", &offer).await.unwrap().unwrap();
            alice.engine.receive_answer("bob", &answer).await.unwrap();
            let mut ready = [false; 4];
            let mut staged = false;
            let mut requested = false;
            let now = RETENTION_MS * 3;
            enum Event { Alice(ChatTransportEvent), Bob(ChatTransportEvent), JobAlice(FileEvent), JobBob(FileEvent) }
            loop {
                for packet in jobs_a.pump(&mut a, Some(&alice), now) { assert!(alice.send_control(&packet)); }
                for packet in jobs_b.pump(&mut b, Some(&bob), now) { assert!(bob.send_control(&packet)); }
                if ready.iter().all(|ready| *ready) && !staged {
                    jobs_a.stage("bob".into(), source.clone()).unwrap(); staged = true;
                }
                if !requested {
                    if let Some(message) = b.messages("alice", now).unwrap().first() {
                        let packet = b.request_file("alice", &message.id, now).unwrap();
                        jobs_b.set_target("alice", &message.id, target.clone());
                        assert!(bob.send_control(&packet)); requested = true;
                    }
                }
                let event = tokio::select! {
                    event = alice.next_event() => Event::Alice(event.unwrap()),
                    event = bob.next_event() => Event::Bob(event.unwrap()),
                    event = async { if jobs_a.has_jobs() { jobs_a.next_event().await } else { std::future::pending().await } } => Event::JobAlice(event.unwrap()),
                    event = async { if jobs_b.has_jobs() { jobs_b.next_event().await } else { std::future::pending().await } } => Event::JobBob(event.unwrap()),
                };
                match event {
                    Event::Alice(ChatTransportEvent::Ice(_, payload)) => bob.engine.receive_ice("alice", payload.strip_prefix("zabor-chat:").unwrap()).await.unwrap(),
                    Event::Bob(ChatTransportEvent::Ice(_, payload)) => alice.engine.receive_ice("bob", payload.strip_prefix("zabor-chat:").unwrap()).await.unwrap(),
                    Event::Alice(ChatTransportEvent::Ready(_)) => { ready[0] = true; assert!(alice.send_control(&a.open("bob").unwrap())); },
                    Event::Bob(ChatTransportEvent::Ready(_)) => { ready[1] = true; assert!(bob.send_control(&b.open("alice").unwrap())); },
                    Event::Alice(ChatTransportEvent::FilesReady(_)) => ready[2] = true,
                    Event::Bob(ChatTransportEvent::FilesReady(_)) => ready[3] = true,
                    Event::Alice(ChatTransportEvent::Text(peer, text)) => { for packet in a.receive_direct(&peer, &text, now).unwrap() { assert!(alice.send_control(&packet)); } },
                    Event::Bob(ChatTransportEvent::Text(peer, text)) => { for packet in b.receive_direct(&peer, &text, now).unwrap() { assert!(bob.send_control(&packet)); } },
                    Event::Bob(ChatTransportEvent::Binary(peer, bytes)) => {
                        let (id, _, complete) = b.receive_file(&peer, &bytes, now).unwrap();
                        if complete { jobs_b.finish_incoming(&b, &peer, &id, now).unwrap(); }
                    },
                    Event::JobAlice(FileEvent::Staged { peer, file }) => {
                        let file = file.unwrap();
                        let packets = a.send_file(&peer, file.info().clone(), now).unwrap(); file.commit();
                        for packet in packets { assert!(alice.send_control(&packet)); }
                    },
                    Event::JobAlice(FileEvent::Sent { success, .. }) => assert!(success),
                    Event::JobBob(FileEvent::Saved { peer, id, stored_name, path }) => {
                        let path = path.unwrap();
                        b.saved_file(&peer, &id, &stored_name, &path, now).unwrap();
                        assert_eq!(std::fs::read(&path).unwrap(), bytes);
                        assert_eq!(std::fs::read(&source).unwrap(), bytes);
                        assert_eq!(b.messages("alice", now).unwrap()[0].file.as_ref().unwrap()["transferState"], "available");
                        assert_eq!(std::fs::read_dir(root.path().join("bob")).unwrap().count(), 1);
                        break;
                    },
                    _ => panic!("unexpected file workflow event"),
                }
            }
        }).await.unwrap();
    }
}
