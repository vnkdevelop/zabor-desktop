use crate::chat::OutboundPacket;
use crate::hub::HubConnection;
use crate::media::{IceServerSettings, MediaEngine, MediaError, MediaEvent};
use libwebrtc::data_channel::{DataChannel, DataChannelInit, DataChannelState};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::mpsc;

pub enum ChatTransportEvent {
    Text(String, String),
    Binary(String, Vec<u8>),
    Ready(String),
    Ice(String, String),
}

pub struct ChatTransport {
    engine: MediaEngine,
    media: mpsc::Receiver<MediaEvent>,
    events: mpsc::Receiver<ChatTransportEvent>,
    sender: mpsc::Sender<ChatTransportEvent>,
    control: BTreeMap<String, DataChannel>,
    files: BTreeMap<String, DataChannel>,
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
        if self
            .control
            .get(peer)
            .is_some_and(|channel| channel.state() == DataChannelState::Open)
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
            self.attach(peer.into(), control);
            self.attach(peer.into(), files);
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
        self.control.get(&packet.peer).is_some_and(|channel| {
            channel.state() == DataChannelState::Open
                && channel.buffered_amount() < 1024 * 1024
                && channel
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
            .filter(|channel| channel.state() == DataChannelState::Open)
            .cloned()
    }

    pub fn set_friends(&mut self, friends: &[String]) {
        self.engine.authorize_peers(friends.iter().cloned());
        for channels in [&mut self.control, &mut self.files] {
            channels.retain(|peer, channel| {
                if friends.contains(peer) {
                    true
                } else {
                    channel.on_message(None);
                    channel.on_state_change(None);
                    channel.close();
                    false
                }
            });
        }
        self.connecting.retain(|peer| friends.contains(peer));
    }

    pub async fn next_event(&mut self) -> Option<ChatTransportEvent> {
        loop {
            tokio::select! {
                event = self.events.recv() => return event,
                event = self.media.recv() => match event? {
                    MediaEvent::Ice { peer, payload } => return Some(ChatTransportEvent::Ice(peer, format!("zabor-chat:{payload}"))),
                    MediaEvent::DataChannel { peer, channel } => self.attach(peer, channel),
                    MediaEvent::Connection { peer, state } => {
                        if matches!(state, libwebrtc::peer_connection::PeerConnectionState::Closed | libwebrtc::peer_connection::PeerConnectionState::Failed) { self.connecting.remove(&peer); }
                    },
                    _ => {},
                },
            }
        }
    }

    fn attach(&mut self, peer: String, channel: DataChannel) {
        let file = channel.label() == "zabor-chat-files-v1";
        if !file && channel.label() != "zabor-chat-v1" {
            channel.close();
            return;
        }
        let sender = self.sender.clone();
        let id = peer.clone();
        channel.on_message(Some(Box::new(move |buffer| {
            if buffer.data.len() > 256 * 1024 {
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
            let _ = sender.try_send(event);
        })));
        let sender = self.sender.clone();
        let id = peer.clone();
        channel.on_state_change(Some(Box::new(move |state| {
            if state == DataChannelState::Open && !file {
                let _ = sender.try_send(ChatTransportEvent::Ready(id.clone()));
            }
        })));
        if channel.state() == DataChannelState::Open && !file {
            let _ = self
                .sender
                .try_send(ChatTransportEvent::Ready(peer.clone()));
        }
        let previous = if file {
            self.files.insert(peer, channel)
        } else {
            self.control.insert(peer, channel)
        };
        if let Some(previous) = previous {
            previous.on_message(None);
            previous.on_state_change(None);
            previous.close();
        }
    }
}

impl Drop for ChatTransport {
    fn drop(&mut self) {
        for channel in self.control.values().chain(self.files.values()) {
            channel.on_message(None);
            channel.on_state_change(None);
            channel.close();
        }
        self.engine.close();
    }
}
