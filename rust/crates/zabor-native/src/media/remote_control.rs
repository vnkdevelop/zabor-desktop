use super::*;
use libwebrtc::data_channel::DataChannelState;
use zabor_input::{Packet, CHANNEL_ID, CHANNEL_LABEL};

#[derive(Clone)]
pub struct ChannelLease(Arc<AtomicBool>);

impl ChannelLease {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
    pub fn is_alive(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn revoke(&self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Clone)]
pub enum RemoteControlEvent {
    Packet(Packet),
    State(DataChannelState),
}

pub(super) struct RemoteChannel {
    channel: DataChannel,
    lease: ChannelLease,
    permitted: bool,
    version: u8,
}

impl RemoteChannel {
    pub fn new(
        connection: &PeerConnection,
        peer: &str,
        lifetime: &PeerLifetime,
        events: mpsc::Sender<MediaEvent>,
    ) -> Result<Self, MediaError> {
        let channel = connection
            .create_data_channel(
                CHANNEL_LABEL,
                DataChannelInit {
                    ordered: true,
                    negotiated: true,
                    id: CHANNEL_ID,
                    ..Default::default()
                },
            )
            .map_err(|_| MediaError::Backend)?;
        let result = Self {
            channel,
            lease: ChannelLease::new(),
            permitted: false,
            version: 1,
        };
        let id = peer.to_owned();
        let original = lifetime.clone();
        let lease = result.lease.clone();
        let states = events.clone();
        result.channel.on_state_change(Some(Box::new(move |state| {
            if original.is_alive()
                && lease.is_alive()
                && states
                    .try_send(MediaEvent::RemoteControl {
                        peer: id.clone(),
                        event: RemoteControlEvent::State(state),
                        lifetime: original.clone(),
                        channel: lease.clone(),
                    })
                    .is_err()
            {
                lease.revoke();
            }
            if matches!(state, DataChannelState::Closed | DataChannelState::Closing) {
                lease.revoke();
            }
        })));
        let id = peer.to_owned();
        let original = lifetime.clone();
        let lease = result.lease.clone();
        result.channel.on_message(Some(Box::new(move |buffer| {
            if !buffer.binary || !original.is_alive() || !lease.is_alive() {
                return;
            }
            if let Some(packet) = Packet::parse(buffer.data) {
                if events
                    .try_send(MediaEvent::RemoteControl {
                        peer: id.clone(),
                        event: RemoteControlEvent::Packet(packet),
                        lifetime: original.clone(),
                        channel: lease.clone(),
                    })
                    .is_err()
                {
                    lease.revoke();
                }
            }
        })));
        Ok(result)
    }

    pub(super) fn metadata(&mut self, value: &Value) {
        self.permitted = value.get("allowRemoteControl").and_then(Value::as_bool) == Some(true);
        self.version = if value
            .get("remoteControlProtocol")
            .and_then(Value::as_u64)
            .is_some_and(|version| version >= 2)
        {
            2
        } else {
            1
        };
    }
}

impl Drop for RemoteChannel {
    fn drop(&mut self) {
        self.lease.revoke();
        self.channel.on_message(None);
        self.channel.on_state_change(None);
        self.channel.close();
    }
}

impl MediaEngine {
    pub fn set_remote_control_allowed(&mut self, allowed: bool) {
        self.remote_allowed =
            allowed && self.has_live_video() && self.video_quality() != Some("camera");
    }

    pub fn remote_control_info(
        &self,
        id: &str,
    ) -> Option<(PeerLifetime, ChannelLease, bool, u8, DataChannelState)> {
        let peer = self.peers.get(id)?;
        let channel = peer.remote_control.as_ref()?;
        Some((
            peer.lifetime.clone(),
            channel.lease.clone(),
            channel.permitted,
            channel.version,
            channel.channel.state(),
        ))
    }

    pub fn remote_control_matches(
        &self,
        id: &str,
        lifetime: &PeerLifetime,
        channel: &ChannelLease,
    ) -> bool {
        self.remote_control_info(id)
            .is_some_and(|(original, lease, _, _, _)| {
                lifetime.is_alive()
                    && original.is_same(lifetime)
                    && lease.is_alive()
                    && lease.is_same(channel)
            })
    }

    pub(crate) fn remote_control_wire(&self, id: &str) -> Option<DataChannel> {
        self.peers
            .get(id)?
            .remote_control
            .as_ref()
            .map(|channel| channel.channel.clone())
    }

    pub(crate) fn maintain_remote_channels(&mut self) -> Vec<String> {
        let ids = self
            .peers
            .iter()
            .filter(|(_, peer)| {
                !self.data_only
                    && peer
                        .remote_control
                        .as_ref()
                        .is_none_or(|channel| !channel.lease.is_alive())
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut repaired = Vec::new();
        for id in ids {
            let Some(peer) = self.peers.get_mut(&id) else {
                continue;
            };
            peer.remote_control = None;
            if let Ok(channel) =
                RemoteChannel::new(&peer.connection, &id, &peer.lifetime, self.events.clone())
            {
                peer.remote_control = Some(channel);
                repaired.push(id);
            }
        }
        repaired
    }

    #[cfg(test)]
    pub(crate) fn test_remote_metadata(&mut self, id: &str, allowed: bool, version: u8) {
        self.peers
            .get_mut(id)
            .unwrap()
            .remote_control
            .as_mut()
            .unwrap()
            .metadata(&json!({"allowRemoteControl":allowed,"remoteControlProtocol":version}));
    }

    pub fn send_remote_control(&self, id: &str, packet: Packet) -> Result<(), MediaError> {
        let peer = self
            .peers
            .get(id)
            .filter(|peer| peer.lifetime.is_alive())
            .ok_or(MediaError::UnauthorizedPeer)?;
        let remote = peer
            .remote_control
            .as_ref()
            .filter(|remote| {
                remote.lease.is_alive() && remote.channel.state() == DataChannelState::Open
            })
            .ok_or(MediaError::Backend)?;
        if matches!(packet, Packet::Move { .. }) && remote.channel.buffered_amount() > 2048 {
            return Ok(());
        }
        remote
            .channel
            .send(&packet.encode(), true)
            .map_err(|_| MediaError::Backend)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use libwebrtc::video_source::VideoResolution;
    use std::time::Duration;
    use zabor_input::Signal;

    pub(crate) async fn connected() -> (
        MediaEngine,
        mpsc::Receiver<MediaEvent>,
        MediaEngine,
        mpsc::Receiver<MediaEvent>,
    ) {
        let (mut host, mut host_events) = MediaEngine::new("owner".into());
        let (mut viewer, mut viewer_events) = MediaEngine::new("peer".into());
        host.authorize_peers(["peer".into()]);
        viewer.authorize_peers(["owner".into()]);
        let offer = host.offer("peer", false).await.unwrap();
        let answer = viewer
            .receive_offer("owner", &offer)
            .await
            .unwrap()
            .unwrap();
        host.receive_answer("peer", &answer).await.unwrap();
        tokio::time::timeout(Duration::from_secs(8), async {
            while host.remote_control_info("peer").unwrap().4 != DataChannelState::Open || viewer.remote_control_info("owner").unwrap().4 != DataChannelState::Open {
                tokio::select! {
                    event = host_events.recv() => { if let Some(MediaEvent::Ice { payload, .. }) = event { viewer.receive_ice("owner", &payload).await.unwrap(); } },
                    event = viewer_events.recv() => { if let Some(MediaEvent::Ice { payload, .. }) = event { host.receive_ice("peer", &payload).await.unwrap(); } },
                }
            }
        }).await.unwrap();
        (host, host_events, viewer, viewer_events)
    }

    #[tokio::test]
    async fn negotiated_channel_delivers_exact_binary_packets_in_order_without_microphone() {
        let (host, _host_events, mut viewer, mut events) = connected().await;
        let channel = host.remote_control_wire("peer").unwrap();
        assert_eq!(channel.id(), CHANNEL_ID);
        assert_eq!(channel.label(), CHANNEL_LABEL);
        let packets = [
            Packet::Signal(Signal::Start),
            Packet::Move { x: 0.25, y: 0.75 },
            Packet::Key {
                vk: 65,
                down: true,
                modifiers: 0,
            },
            Packet::Key {
                vk: 65,
                down: false,
                modifiers: 0,
            },
            Packet::Signal(Signal::Stop),
        ];
        channel.send(&[0x16], false).unwrap();
        channel.send(&[1, 0], true).unwrap();
        for packet in packets {
            host.send_remote_control("peer", packet).unwrap();
        }
        let received = tokio::time::timeout(Duration::from_secs(5), async {
            let mut received = Vec::new();
            while received.len() < packets.len() {
                if let Some(MediaEvent::RemoteControl {
                    event: RemoteControlEvent::Packet(packet),
                    lifetime,
                    channel,
                    ..
                }) = events.recv().await
                {
                    assert!(viewer.remote_control_matches("owner", &lifetime, &channel));
                    received.push(packet);
                }
            }
            received
        })
        .await
        .unwrap();
        assert_eq!(received, packets);
        assert!(host.microphone.is_none() && viewer.microphone.is_none());
        viewer.close();
    }

    #[tokio::test]
    async fn metadata_requires_true_boolean_and_current_accepted_sdp_and_cameras_never_advertise_consent(
    ) {
        let (mut host, _) = MediaEngine::new("owner".into());
        host.authorize_peers(["peer".into()]);
        let source = NativeVideoSource::new(
            VideoResolution {
                width: 160,
                height: 96,
            },
            true,
        );
        host.set_video_with_quality(source.clone(), "high").unwrap();
        host.set_remote_control_allowed(true);
        let offer = host.offer("peer", false).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&offer).unwrap()["allowRemoteControl"],
            true
        );
        let peer = host.peers.get_mut("peer").unwrap();
        let remote = peer.remote_control.as_mut().unwrap();
        for value in [
            json!({}),
            json!({"allowRemoteControl":"true"}),
            json!({"allowRemoteControl":1}),
        ] {
            remote.metadata(&value);
            assert!(!remote.permitted);
        }
        remote.metadata(&json!({"allowRemoteControl":true,"remoteControlProtocol":2}));
        let mut stale: Value = serde_json::from_str(&offer).unwrap();
        stale["epoch"] = json!(0);
        stale["allowRemoteControl"] = json!(false);
        stale["sdp"] = json!("not sdp");
        host.receive_answer("peer", &stale.to_string())
            .await
            .unwrap();
        assert!(host.remote_control_info("peer").unwrap().2);
        assert_eq!(host.remote_control_info("peer").unwrap().3, 2);
        host.disconnect_peer("peer");
        host.set_video_with_quality(source, "camera").unwrap();
        host.set_remote_control_allowed(true);
        assert_eq!(
            serde_json::from_str::<Value>(&host.offer("peer", false).await.unwrap()).unwrap()
                ["allowRemoteControl"],
            false
        );
    }

    #[tokio::test]
    async fn original_channel_and_peer_retirement_reject_replaced_same_peer_callbacks() {
        let (mut host, _) = MediaEngine::new("owner".into());
        host.authorize_peers(["peer".into()]);
        host.peer("peer").unwrap();
        let (lifetime, channel, _, _, _) = host.remote_control_info("peer").unwrap();
        channel.revoke();
        assert_eq!(host.maintain_remote_channels(), ["peer"]);
        assert!(!host.remote_control_matches("peer", &lifetime, &channel));
        let (_, fresh, _, _, _) = host.remote_control_info("peer").unwrap();
        assert!(!fresh.is_same(&channel) && fresh.is_alive());
        host.disconnect_peer("peer");
        assert!(!lifetime.is_alive() && !fresh.is_alive());
        assert!(!host.remote_control_matches("peer", &lifetime, &fresh));
    }

    #[tokio::test]
    async fn controller_overrides_keep_p2p_full_resolution_and_voice_after_sfu_ready_then_restore_preview(
    ) {
        let (mut host, _) = MediaEngine::new("owner".into());
        host.authorize_peers(["peer".into()]);
        host.attach_audio_source(crate::microphone::audio_source())
            .unwrap();
        host.set_video_with_quality(
            NativeVideoSource::new(
                VideoResolution {
                    width: 160,
                    height: 96,
                },
                true,
            ),
            "high",
        )
        .unwrap();
        host.peer("peer").unwrap();
        host.set_remote_control_allowed(true);
        let lifetime = host.peer_lifetime("peer").unwrap();
        host.set_viewer_state("peer", ViewerState::Preview).unwrap();
        host.begin_sfu_publisher(42).unwrap();
        host.set_delivery_mode(DeliveryMode::Sfu, 42).unwrap();
        host.viewer_sfu_ready("peer", 42, false).unwrap();
        host.set_controller("peer", true).unwrap();
        host.viewer_sfu_ready("peer", 42, false).unwrap();
        let encoding = host.peers["peer"]
            .video_sender
            .as_ref()
            .unwrap()
            .parameters()
            .encodings[0]
            .clone();
        assert!(encoding.active);
        assert_eq!(encoding.scale_resolution_down_by, Some(1.0));
        assert_eq!(encoding.max_framerate, Some(60.0));
        assert!(host.microphone.as_ref().unwrap().enabled() && lifetime.is_alive());
        host.set_controller("peer", false).unwrap();
        assert!(
            !host.peers["peer"]
                .video_sender
                .as_ref()
                .unwrap()
                .parameters()
                .encodings[0]
                .active
        );
    }
}
