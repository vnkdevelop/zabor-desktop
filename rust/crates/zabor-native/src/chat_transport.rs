use crate::chat::OutboundPacket;
use crate::hub::HubConnection;
use crate::media::{IceServerSettings, MediaEngine, MediaError, MediaEvent, PeerLifetime};
use libwebrtc::data_channel::{DataChannel, DataChannelInit, DataChannelState};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::mpsc;

pub enum ChatTransportEvent {
    Text(String, String),
    Binary(String, Vec<u8>),
    Ready(String),
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
    connecting: BTreeSet<String>,
}

impl ChatTransport {
    pub fn new(
        owner: String,
        friends: impl IntoIterator<Item = String>,
        ice: Vec<IceServerSettings>,
        relay: bool,
    ) -> Result<Self, MediaError> {
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
            connecting: BTreeSet::new(),
        })
    }

    pub async fn connect(&mut self, hub: &HubConnection, peer: &str) -> Result<(), MediaError> {
        if self.control.get(peer).is_some_and(ChannelBinding::is_open)
            || !self.connecting.insert(peer.into())
        {
            return Ok(());
        }
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
        self.control.get(&packet.peer).is_some_and(|binding| {
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
        self.files
            .get(peer)
            .filter(|binding| binding.is_open())
            .map(|binding| binding.channel.clone())
    }

    pub fn set_friends(&mut self, friends: &[String]) {
        self.engine.authorize_peers(friends.iter().cloned());
        for channels in [&mut self.control, &mut self.files] {
            channels.retain(|peer, _| friends.contains(peer));
        }
        self.connecting.retain(|peer| friends.contains(peer));
    }

    pub async fn next_event(&mut self) -> Option<ChatTransportEvent> {
        loop {
            tokio::select! {
                event = self.events.recv() => {
                    let event = event?;
                    if event.lifetime.is_alive() && event.active.load(Ordering::Acquire) { return Some(event.event); }
                },
                event = self.media.recv() => { let event = event?; if !event.is_alive() { continue; } match event {
                    MediaEvent::Ice { peer, payload, .. } => return Some(ChatTransportEvent::Ice(peer, format!("zabor-chat:{payload}"))),
                    MediaEvent::DataChannel { peer, channel, lifetime } => self.attach(peer, channel, lifetime),
                    MediaEvent::Connection { peer, state, .. } => {
                        if matches!(state, libwebrtc::peer_connection::PeerConnectionState::Closed | libwebrtc::peer_connection::PeerConnectionState::Failed) { self.connecting.remove(&peer); }
                    },
                    _ => {},
                } },
            }
        }
    }

    fn attach(&mut self, peer: String, channel: DataChannel, lifetime: PeerLifetime) {
        let file = channel.label() == "zabor-chat-files-v1";
        if !file && channel.label() != "zabor-chat-v1" {
            channel.close();
            return;
        }
        let sender = self.sender.clone();
        let id = peer.clone();
        let active = Arc::new(AtomicBool::new(true));
        let live = lifetime.clone();
        let callback_active = active.clone();
        channel.on_message(Some(Box::new(move |buffer| {
            if !live.is_alive()
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
            let _ = sender.try_send(TransportMessage {
                event,
                lifetime: live.clone(),
                active: callback_active.clone(),
            });
        })));
        let sender = self.sender.clone();
        let id = peer.clone();
        let live = lifetime.clone();
        let callback_active = active.clone();
        channel.on_state_change(Some(Box::new(move |state| {
            if state == DataChannelState::Open
                && !file
                && live.is_alive()
                && callback_active.load(Ordering::Acquire)
            {
                let _ = sender.try_send(TransportMessage {
                    event: ChatTransportEvent::Ready(id.clone()),
                    lifetime: live.clone(),
                    active: callback_active.clone(),
                });
            }
        })));
        if channel.state() == DataChannelState::Open && !file {
            let _ = self.sender.try_send(TransportMessage {
                event: ChatTransportEvent::Ready(peer.clone()),
                lifetime: lifetime.clone(),
                active: active.clone(),
            });
        }
        let binding = ChannelBinding {
            channel,
            active,
            lifetime,
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
}
