use super::*;
use crate::media::{ChannelLease, RemoteControlEvent};
use crate::remote_input::{InputState, RemoteInput};
use libwebrtc::data_channel::DataChannelState;
use tokio_util::sync::CancellationToken;
use zabor_input::{Packet, Signal};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteControlView {
    pub permitted_peers: Vec<String>,
    pub controller: String,
    pub viewer: String,
    pub viewer_ready: bool,
    pub host_active: bool,
    pub revoked: bool,
    pub notice: Option<&'static str>,
}

#[derive(Clone)]
pub struct ControlTarget {
    pub peer: String,
    pub lifetime: PeerLifetime,
    pub channel: ChannelLease,
    wire: libwebrtc::data_channel::DataChannel,
}

impl ControlTarget {
    pub fn send_stop(&self) {
        if self.lifetime.is_alive() && self.channel.is_alive() {
            let _ = self.wire.send(&Packet::Signal(Signal::Stop).encode(), true);
        }
    }
    pub fn emergency_stop(&self) {
        self.send_stop();
        self.channel.revoke();
    }
}

struct Viewer {
    peer: String,
    lifetime: PeerLifetime,
    channel: ChannelLease,
    accepted: bool,
    requested: std::time::Instant,
    ping: std::time::Instant,
    host_active: bool,
}

struct Host {
    peer: String,
    lifetime: PeerLifetime,
    channel: ChannelLease,
    input: RemoteInput,
    last_state: InputState,
}

#[derive(Default)]
pub(super) struct RemoteControl {
    pub(super) allowed: bool,
    pub(super) revoked: bool,
    host: Option<Host>,
    viewer: Option<Viewer>,
    notice: Option<(std::time::Instant, &'static str)>,
    #[cfg(test)]
    synthetic_input: bool,
}

impl VoiceSession {
    pub fn remote_control_view(&self) -> RemoteControlView {
        let host = self.remote.host.as_ref();
        let viewer = self.remote.viewer.as_ref();
        RemoteControlView {
            permitted_peers: self
                .peers
                .iter()
                .filter(|peer| {
                    self.engine
                        .remote_control_info(peer)
                        .is_some_and(|(_, channel, allowed, _, _)| channel.is_alive() && allowed)
                })
                .cloned()
                .collect(),
            controller: host.map(|host| host.peer.clone()).unwrap_or_default(),
            viewer: viewer.map(|viewer| viewer.peer.clone()).unwrap_or_default(),
            viewer_ready: viewer
                .is_some_and(|viewer| viewer.accepted && self.control_p2p_ready(&viewer.peer)),
            host_active: host.is_some_and(|host| host.input.state() == InputState::HostActive)
                || viewer.is_some_and(|viewer| viewer.host_active),
            revoked: self.remote.revoked,
            notice: self
                .remote
                .notice
                .filter(|(at, _)| at.elapsed() < Duration::from_secs(3))
                .map(|(_, key)| key),
        }
    }

    pub fn control_targets(&self) -> Vec<ControlTarget> {
        self.peers
            .iter()
            .filter_map(|peer| {
                let (lifetime, channel, _, _, _) = self.engine.remote_control_info(peer)?;
                Some(ControlTarget {
                    peer: peer.clone(),
                    lifetime,
                    channel,
                    wire: self.engine.remote_control_wire(peer)?,
                })
            })
            .collect()
    }

    pub fn control_target_current(&self, target: &ControlTarget) -> bool {
        self.engine
            .remote_control_matches(&target.peer, &target.lifetime, &target.channel)
    }

    pub(super) fn reset_remote_control(&mut self) {
        self.stop_host_control(false);
        if let Some(viewer) = self.remote.viewer.take() {
            let _ = self
                .engine
                .send_remote_control(&viewer.peer, Packet::Signal(Signal::Stop));
        }
        self.set_control_stream(None);
    }

    fn stop_host_control(&mut self, permanent: bool) {
        self.remote.revoked = permanent;
        if let Some(host) = self.remote.host.take() {
            host.input.stop();
            let _ = self.engine.send_remote_control(
                &host.peer,
                Packet::Signal(if permanent {
                    Signal::HostRevoked
                } else {
                    Signal::Reset
                }),
            );
            let _ = self.engine.set_controller(&host.peer, false);
            drop(host);
        }
    }

    pub fn revoke_remote_control(&mut self) {
        self.remote.allowed = false;
        self.engine.set_remote_control_allowed(false);
        self.stop_host_control(true);
    }

    pub fn release_remote_control(&mut self) {
        self.reset_remote_control();
    }

    async fn open_control_input(
        &self,
        geometry: Arc<zabor_input::CaptureLease>,
        voice: CancellationToken,
        lifetime: PeerLifetime,
        channel: ChannelLease,
    ) -> Result<RemoteInput, MediaError> {
        #[cfg(test)]
        if self.remote.synthetic_input {
            return RemoteInput::start_with(
                self.audio_runtime.cleanup_tracker(),
                voice,
                move |receiver, stop, status| {
                    if stop.is_cancelled()
                        || geometry.snapshot().is_none()
                        || !lifetime.is_alive()
                        || !channel.is_alive()
                    {
                        return Err(zabor_input::InputError::Unavailable);
                    }
                    status.send_replace(InputState::Ready);
                    while !stop.is_cancelled() && lifetime.is_alive() && channel.is_alive() {
                        if let Ok(Packet::Signal(Signal::Reset | Signal::Stop)) =
                            receiver.recv_timeout(Duration::from_millis(5))
                        {
                            break;
                        }
                    }
                    Ok(())
                },
            )
            .await;
        }
        RemoteInput::start(
            self.audio_runtime.cleanup_tracker(),
            geometry,
            voice,
            lifetime,
            channel,
        )
        .await
    }

    pub async fn toggle_remote_control(
        &mut self,
        hub: &HubConnection,
        peer: &str,
    ) -> Result<(), MediaError> {
        if self
            .remote
            .viewer
            .as_ref()
            .is_some_and(|viewer| viewer.peer == peer)
        {
            self.reset_remote_control();
            return Ok(());
        }
        let (lifetime, channel, permitted, _, state) = self
            .engine
            .remote_control_info(peer)
            .ok_or(MediaError::UnauthorizedPeer)?;
        if !self.peers.contains(peer)
            || !permitted
            || !lifetime.is_alive()
            || !channel.is_alive()
            || !self.control_stream_focused(peer)
            || !matches!(state, DataChannelState::Connecting | DataChannelState::Open)
        {
            return Err(MediaError::UnauthorizedPeer);
        }
        self.reset_remote_control();
        self.remote.revoked = false;
        self.remote.notice = None;
        self.remote.viewer = Some(Viewer {
            peer: peer.into(),
            lifetime,
            channel,
            accepted: false,
            requested: std::time::Instant::now(),
            ping: std::time::Instant::now(),
            host_active: false,
        });
        if state == DataChannelState::Open {
            self.engine
                .send_remote_control(peer, Packet::Signal(Signal::Start))?;
        }
        self.set_stream_focus(hub, Some(peer.into())).await
    }

    pub fn send_control_input(&mut self, peer: &str, packet: Packet) -> Result<(), MediaError> {
        let Some(viewer) = self
            .remote
            .viewer
            .as_ref()
            .filter(|viewer| viewer.peer == peer && viewer.accepted)
        else {
            return Err(MediaError::UnauthorizedPeer);
        };
        if !self
            .engine
            .remote_control_matches(peer, &viewer.lifetime, &viewer.channel)
            || !self.control_p2p_ready(peer)
            || !self
                .engine
                .remote_control_info(peer)
                .is_some_and(|(_, _, allowed, _, _)| allowed)
        {
            return Err(MediaError::UnauthorizedPeer);
        }
        if viewer.host_active && !packet.is_release() {
            return Ok(());
        }
        if !matches!(
            packet,
            Packet::Move { .. } | Packet::Mouse { .. } | Packet::Wheel { .. } | Packet::Key { .. }
        ) || Packet::parse(&packet.encode()).is_none()
        {
            return Err(MediaError::InvalidSignal);
        }
        if let Packet::Key {
            vk,
            down: true,
            modifiers,
        } = packet
        {
            if !zabor_input::key_safe(vk, modifiers) {
                return Ok(());
            }
        }
        if self.engine.send_remote_control(peer, packet).is_err() {
            self.reset_remote_control();
            return Err(MediaError::Backend);
        }
        Ok(())
    }

    pub async fn handle_remote_control(
        &mut self,
        hub: &HubConnection,
        peer: &str,
        event: RemoteControlEvent,
        lifetime: PeerLifetime,
        channel: ChannelLease,
        voice: CancellationToken,
    ) -> Result<(), MediaError> {
        if voice.is_cancelled()
            || !self.peers.contains(peer)
            || !self
                .engine
                .remote_control_matches(peer, &lifetime, &channel)
        {
            return Ok(());
        }
        if let RemoteControlEvent::State(state) = event {
            if state == DataChannelState::Open
                && self
                    .remote
                    .viewer
                    .as_ref()
                    .is_some_and(|viewer| viewer.peer == peer)
            {
                self.engine
                    .send_remote_control(peer, Packet::Signal(Signal::Start))?;
            } else if matches!(state, DataChannelState::Closing | DataChannelState::Closed) {
                if self
                    .remote
                    .host
                    .as_ref()
                    .is_some_and(|host| host.peer == peer)
                {
                    self.stop_host_control(false);
                }
                if self
                    .remote
                    .viewer
                    .as_ref()
                    .is_some_and(|viewer| viewer.peer == peer)
                {
                    self.reset_remote_control();
                }
            }
            return Ok(());
        }
        let RemoteControlEvent::Packet(packet) = event else {
            return Ok(());
        };
        let version = self
            .engine
            .remote_control_info(peer)
            .map(|(_, _, _, version, _)| version)
            .unwrap_or(1);
        if self
            .remote
            .viewer
            .as_ref()
            .is_some_and(|viewer| viewer.peer == peer)
        {
            match packet {
                Packet::Signal(Signal::Accept) | Packet::Signal(Signal::Start)
                    if packet != Packet::Signal(Signal::Start) || version < 2 =>
                {
                    if self
                        .remote
                        .viewer
                        .as_ref()
                        .is_some_and(|viewer| viewer.accepted)
                    {
                        return Ok(());
                    }
                    if let Some(viewer) = self.remote.viewer.as_mut() {
                        viewer.accepted = true;
                    }
                    self.set_control_stream(Some(peer.into()));
                    self.control_to_p2p(hub, peer).await;
                }
                Packet::Signal(Signal::HostActive) => {
                    if let Some(viewer) = self.remote.viewer.as_mut() {
                        viewer.host_active = true;
                    }
                }
                Packet::Signal(Signal::HostIdle) => {
                    if let Some(viewer) = self.remote.viewer.as_mut() {
                        viewer.host_active = false;
                    }
                }
                Packet::Signal(
                    Signal::HostRevoked | Signal::Busy | Signal::Reset | Signal::Stop,
                ) => {
                    self.reset_remote_control();
                    self.remote.revoked = packet == Packet::Signal(Signal::HostRevoked);
                    if packet == Packet::Signal(Signal::Busy) {
                        self.remote.notice =
                            Some((std::time::Instant::now(), "stream.controlBusy"));
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        if packet == Packet::Signal(Signal::Start) {
            let refused = if self.remote.viewer.is_some()
                || self
                    .remote
                    .host
                    .as_ref()
                    .is_some_and(|host| host.peer != peer)
            {
                Some(Signal::Busy)
            } else if !self.remote.allowed || self.remote.revoked || !self.is_sharing() {
                Some(Signal::HostRevoked)
            } else {
                None
            };
            if let Some(signal) = refused {
                return self
                    .engine
                    .send_remote_control(peer, Packet::Signal(signal));
            }
            if self.remote.host.is_none() {
                let geometry = self
                    .capture
                    .as_ref()
                    .and_then(|share| match &share.capture {
                        ShareCapture::Desktop(capture) => Some(capture.geometry.clone()),
                        ShareCapture::Camera(_) => None,
                    })
                    .ok_or(MediaError::Backend)?;
                let input = match self
                    .open_control_input(geometry, voice.clone(), lifetime.clone(), channel.clone())
                    .await
                {
                    Ok(input) => input,
                    Err(_) => {
                        return self
                            .engine
                            .send_remote_control(peer, Packet::Signal(Signal::HostRevoked))
                    }
                };
                if voice.is_cancelled()
                    || !self
                        .engine
                        .remote_control_matches(peer, &lifetime, &channel)
                {
                    drop(input);
                    return Ok(());
                }
                if self.engine.set_controller(peer, true).is_err() {
                    drop(input);
                    return Err(MediaError::Backend);
                }
                self.remote.host = Some(Host {
                    peer: peer.into(),
                    lifetime,
                    channel,
                    input,
                    last_state: InputState::Ready,
                });
            }
            if self
                .engine
                .send_remote_control(
                    peer,
                    Packet::Signal(if version >= 2 {
                        Signal::Accept
                    } else {
                        Signal::Start
                    }),
                )
                .is_err()
            {
                self.stop_host_control(false);
            }
        } else if packet == Packet::Signal(Signal::KeyframeRequest) && self.is_sharing() {
            self.renegotiate(hub, peer).await?;
        } else if self.remote.host.as_ref().is_some_and(|host| {
            host.peer == peer
                && (matches!(packet, Packet::Signal(Signal::Reset | Signal::Stop))
                    || !host.input.feed(packet))
        }) {
            self.stop_host_control(false);
        }
        let _ = hub;
        Ok(())
    }

    pub(super) fn maintain_remote_control(&mut self) {
        let mut host_signal = None;
        let stop_host = self.remote.host.as_ref().is_some_and(|host| {
            let state = host.input.state();
            host_signal = match state {
                InputState::HostActive => Some(Signal::HostActive),
                InputState::HostIdle => Some(Signal::HostIdle),
                _ => None,
            };
            !self
                .engine
                .remote_control_matches(&host.peer, &host.lifetime, &host.channel)
                || !self.remote.allowed
                || !self.is_sharing()
                || matches!(state, InputState::Ended | InputState::Revoked)
        });
        if stop_host {
            self.stop_host_control(true);
        } else if let (Some(host), Some(signal)) = (&mut self.remote.host, host_signal) {
            let state = host.input.state();
            if state != host.last_state {
                host.last_state = state;
                let _ = self
                    .engine
                    .send_remote_control(&host.peer, Packet::Signal(signal));
            }
        }
        let stop_viewer = self.remote.viewer.as_ref().is_some_and(|viewer| {
            !self
                .engine
                .remote_control_matches(&viewer.peer, &viewer.lifetime, &viewer.channel)
                || !self
                    .engine
                    .remote_control_info(&viewer.peer)
                    .is_some_and(|(_, _, permitted, _, _)| permitted)
                || !self.control_stream_focused(&viewer.peer)
                || (!viewer.accepted && viewer.requested.elapsed() >= Duration::from_secs(15))
        });
        if stop_viewer {
            self.reset_remote_control();
        }
        if let Some(viewer) = self.remote.viewer.as_mut() {
            if viewer.ping.elapsed() >= Duration::from_secs(2) {
                viewer.ping = std::time::Instant::now();
                let _ = self
                    .engine
                    .send_remote_control(&viewer.peer, Packet::Signal(Signal::Ping));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::remote_control::tests::connected;
    use crate::voice::stream_tests::{fixture, session, share};
    use libwebrtc::peer_connection_factory::{
        native::PeerConnectionFactoryExt, PeerConnectionFactory,
    };
    use libwebrtc::video_frame::{I420Buffer, VideoFrame, VideoRotation};
    use libwebrtc::video_source::{native::NativeVideoSource, VideoResolution};

    async fn signal(events: &mut mpsc::Receiver<MediaEvent>) -> Signal {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(MediaEvent::RemoteControl {
                    event: RemoteControlEvent::Packet(Packet::Signal(signal)),
                    ..
                }) = events.recv().await
                {
                    return signal;
                }
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn unauthorized_start_has_no_input_owner_and_does_not_close_the_channel_or_voice() {
        let fixture = fixture([]).await;
        let (hub, _hub_events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let (engine, _events, _remote, mut received) = connected().await;
        let mut session = session();
        session.engine = engine;
        let (lifetime, channel, _, _, _) = session.engine.remote_control_info("peer").unwrap();
        session
            .handle_remote_control(
                &hub,
                "peer",
                RemoteControlEvent::Packet(Packet::Signal(Signal::Start)),
                lifetime.clone(),
                channel.clone(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(signal(&mut received).await, Signal::HostRevoked);
        assert!(session.remote.host.is_none());
        assert!(channel.is_alive() && lifetime.is_alive());
        assert_eq!(
            session.engine.remote_control_info("peer").unwrap().4,
            DataChannelState::Open
        );
        session
            .audio_runtime
            .cleanup_tracker()
            .wait()
            .await
            .unwrap();
        hub.close();
        fixture.server.await.unwrap();
    }

    #[tokio::test]
    async fn consent_admits_one_registered_owner_and_acknowledges_v2_or_legacy_only_after_ready() {
        for version in [1, 2] {
            let fixture = fixture([]).await;
            let (hub, _hub_events) = HubConnection::connect(&fixture.endpoint, None)
                .await
                .unwrap();
            let (engine, _events, _remote, mut received) = connected().await;
            let mut session = session();
            session.engine = engine;
            share(&mut session, "high");
            let ShareCapture::Desktop(capture) = &session.capture.as_ref().unwrap().capture else {
                panic!()
            };
            capture.geometry.update(zabor_input::Rect {
                left: -1920,
                top: 0,
                width: 1920,
                height: 1080,
            });
            session.remote.synthetic_input = true;
            session.remote.allowed = true;
            session.engine.set_remote_control_allowed(true);
            session.engine.test_remote_metadata("peer", false, version);
            let (lifetime, channel, _, _, _) = session.engine.remote_control_info("peer").unwrap();
            let voice = CancellationToken::new();
            session
                .handle_remote_control(
                    &hub,
                    "peer",
                    RemoteControlEvent::Packet(Packet::Signal(Signal::Start)),
                    lifetime.clone(),
                    channel.clone(),
                    voice.clone(),
                )
                .await
                .unwrap();
            assert_eq!(
                signal(&mut received).await,
                if version == 2 {
                    Signal::Accept
                } else {
                    Signal::Start
                }
            );
            assert_eq!(
                session.remote.host.as_ref().unwrap().input.state(),
                InputState::Ready
            );
            session.peers.insert("other".into());
            session.engine.authorize_peers(session.peers.clone());
            session
                .engine
                .create_data_channel(
                    "other",
                    "fixture-only",
                    libwebrtc::data_channel::DataChannelInit::default(),
                )
                .unwrap();
            let (other, other_channel, _, _, _) =
                session.engine.remote_control_info("other").unwrap();
            let _ = session
                .handle_remote_control(
                    &hub,
                    "other",
                    RemoteControlEvent::Packet(Packet::Signal(Signal::Start)),
                    other,
                    other_channel,
                    voice.clone(),
                )
                .await;
            assert_eq!(session.remote.host.as_ref().unwrap().peer, "peer");
            voice.cancel();
            session
                .audio_runtime
                .cleanup_tracker()
                .wait()
                .await
                .unwrap();
            session.maintain_remote_control();
            assert!(session.remote.host.is_none() && lifetime.is_alive());
            let cleanup = session.audio_runtime.cleanup_tracker();
            drop(session);
            cleanup.wait().await.unwrap();
            hub.close();
            fixture.server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn viewer_waits_for_correct_ack_and_fresh_p2p_frame_and_repeated_ack_does_not_restart_probe(
    ) {
        let fixture = fixture([]).await;
        let (hub, _hub_events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let (engine, mut events, _remote, _received) = connected().await;
        let mut session = session();
        session.engine = engine;
        session.engine.test_remote_metadata("peer", true, 2);
        session
            .handle(&hub, "UserStartedStreaming", &[json!("peer")])
            .await
            .unwrap();
        session
            .set_stream_focus(&hub, Some("peer".into()))
            .await
            .unwrap();
        session.toggle_remote_control(&hub, "peer").await.unwrap();
        let (lifetime, channel, _, _, _) = session.engine.remote_control_info("peer").unwrap();
        let voice = CancellationToken::new();
        session
            .handle_remote_control(
                &hub,
                "peer",
                RemoteControlEvent::Packet(Packet::Signal(Signal::Start)),
                lifetime.clone(),
                channel.clone(),
                voice.clone(),
            )
            .await
            .unwrap();
        assert!(!session.remote.viewer.as_ref().unwrap().accepted);
        session
            .handle_remote_control(
                &hub,
                "peer",
                RemoteControlEvent::Packet(Packet::Signal(Signal::Accept)),
                lifetime.clone(),
                channel.clone(),
                voice.clone(),
            )
            .await
            .unwrap();
        assert!(!session.remote_control_view().viewer_ready);
        let source = NativeVideoSource::new(
            VideoResolution {
                width: 160,
                height: 96,
            },
            true,
        );
        let track = PeerConnectionFactory::default()
            .create_video_track("remote-control-local-frame", source.clone());
        session.prepare_p2p_video("peer", track, lifetime.clone());
        let ready = tokio::time::timeout(Duration::from_secs(3), async {
            let mut ticker = tokio::time::interval(Duration::from_millis(33));
            loop {
                tokio::select! {
                    _ = ticker.tick() => { source.capture_frame(&VideoFrame::new(VideoRotation::VideoRotation0, I420Buffer::new_black(160,96))); },
                    event = events.recv() => { if let Some(event @ MediaEvent::StreamVideoReady { .. }) = event { return event; } },
                }
            }
        }).await.unwrap();
        session
            .handle_remote_control(
                &hub,
                "peer",
                RemoteControlEvent::Packet(Packet::Signal(Signal::Accept)),
                lifetime.clone(),
                channel.clone(),
                voice.clone(),
            )
            .await
            .unwrap();
        let MediaEvent::StreamVideoReady { lease, .. } = ready else {
            panic!()
        };
        assert!(lease.is_alive());
        assert!(
            session
                .accept_stream_video_ready(&hub, "peer", None, &lifetime, &lease)
                .await
        );
        assert!(session.remote_control_view().viewer_ready);
        session
            .handle_remote_control(
                &hub,
                "peer",
                RemoteControlEvent::Packet(Packet::Signal(Signal::HostActive)),
                lifetime.clone(),
                channel,
                voice,
            )
            .await
            .unwrap();
        assert!(session.remote_control_view().host_active);
        let target = session
            .control_targets()
            .into_iter()
            .find(|target| target.peer == "peer")
            .unwrap();
        session.engine.disconnect_peer("peer");
        assert!(!session.control_target_current(&target));
        assert!(session
            .send_control_input(
                "peer",
                Packet::Key {
                    vk: 65,
                    down: true,
                    modifiers: 0
                }
            )
            .is_err());
        let cleanup = session.audio_runtime.cleanup_tracker();
        drop(session);
        cleanup.wait().await.unwrap();
        hub.close();
        fixture.server.await.unwrap();
    }
}
