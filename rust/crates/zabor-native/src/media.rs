use libwebrtc::audio_source::native::NativeAudioSource;
use libwebrtc::audio_track::RtcAudioTrack;
use libwebrtc::data_channel::{DataChannel, DataChannelInit};
use libwebrtc::ice_candidate::IceCandidate;
use libwebrtc::media_stream_track::MediaStreamTrack;
use libwebrtc::peer_connection::{
    AnswerOptions, OfferOptions, PeerConnection, PeerConnectionState, SignalingState,
};
use libwebrtc::peer_connection_factory::{
    native::PeerConnectionFactoryExt, IceServer, IceTransportsType, PeerConnectionFactory,
    RtcConfiguration,
};
use libwebrtc::rtp_parameters::{
    DegradationPreference, Priority, RtpEncodingParameters, RtpParameters,
};
use libwebrtc::rtp_sender::RtpSender;
use libwebrtc::rtp_transceiver::{RtpTransceiverDirection, RtpTransceiverInit};
use libwebrtc::session_description::{SdpType, SessionDescription};
use libwebrtc::video_source::native::NativeVideoSource;
use libwebrtc::video_track::RtcVideoTrack;
use libwebrtc::MediaType;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

static NEXT_OFFER_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    #[error("peer is outside the active media session")]
    UnauthorizedPeer,
    #[error("invalid media signaling message")]
    InvalidSignal,
    #[error("native media operation failed")]
    Backend,
    #[error("microphone is unavailable")]
    Microphone,
    #[error("channel is full")]
    ChannelFull,
}

#[derive(Clone)]
pub struct PeerLifetime(Arc<PeerState>);

struct PeerState {
    alive: AtomicBool,
    stop: CancellationToken,
    commit: Mutex<()>,
}

impl PeerLifetime {
    fn new() -> Self {
        Self(Arc::new(PeerState {
            alive: AtomicBool::new(true),
            stop: CancellationToken::new(),
            commit: Mutex::new(()),
        }))
    }
    pub fn is_alive(&self) -> bool {
        self.0.alive.load(Ordering::Acquire)
    }
    pub fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn commit<T>(&self, mut accepted: impl FnMut() -> T) -> Option<T> {
        let _gate = self.0.commit.lock().ok()?;
        self.is_alive().then(&mut accepted)
    }
    pub async fn cancelled(&self) {
        self.0.stop.cancelled().await;
    }
    fn invalidate(&self) {
        {
            let _gate = self
                .0
                .commit
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.0.alive.store(false, Ordering::Release);
        }
        self.0.stop.cancel();
    }
}

#[derive(Clone)]
pub enum MediaEvent {
    Ice {
        peer: String,
        payload: String,
        lifetime: PeerLifetime,
    },
    Connection {
        peer: String,
        state: PeerConnectionState,
        lifetime: PeerLifetime,
    },
    Track {
        peer: String,
        track: MediaStreamTrack,
        lifetime: PeerLifetime,
    },
    DataChannel {
        peer: String,
        channel: DataChannel,
        lifetime: PeerLifetime,
    },
    Renegotiate {
        peer: String,
        lifetime: PeerLifetime,
    },
}

impl MediaEvent {
    pub fn is_alive(&self) -> bool {
        self.lifetime().is_alive()
    }

    pub fn lifetime(&self) -> &PeerLifetime {
        match self {
            Self::Ice { lifetime, .. }
            | Self::Connection { lifetime, .. }
            | Self::Track { lifetime, .. }
            | Self::DataChannel { lifetime, .. }
            | Self::Renegotiate { lifetime, .. } => lifetime,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct IceServerSettings {
    pub urls: Vec<String>,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub credential: String,
}

struct Peer {
    connection: PeerConnection,
    lifetime: PeerLifetime,
    epoch: u64,
    ignored_offer: bool,
    pending_ice: VecDeque<IceCandidate>,
    video_sender: Option<RtpSender>,
    video_defaults: Option<VideoSendDefaults>,
    needs_offer: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VideoSendProfile {
    Low,
    Medium,
    High,
    Camera,
}

impl VideoSendProfile {
    fn parse(quality: &str) -> Result<Self, MediaError> {
        match quality {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "camera" => Ok(Self::Camera),
            _ => Err(MediaError::Backend),
        }
    }

    fn apply(self, parameters: &mut RtpParameters) {
        let (bitrate, fps) = match self {
            Self::Low => (2_500_000, 30.0),
            Self::Medium => (3_500_000, 60.0),
            Self::High => (6_000_000, 60.0),
            Self::Camera => return,
        };
        if parameters.encodings.is_empty() {
            parameters.encodings.push(RtpEncodingParameters::default());
        }
        parameters.set_degradation_preference(DegradationPreference::MaintainFramerate);
        parameters.encodings[0].max_bitrate = Some(bitrate);
        parameters.encodings[0].max_framerate = Some(fps);
        parameters.encodings[0].priority = Priority::High;
    }

    fn configure(self, sender: &RtpSender) -> Result<Option<VideoSendDefaults>, MediaError> {
        if self == Self::Camera {
            return Ok(None);
        }
        let mut parameters = sender.parameters();
        let defaults = VideoSendDefaults::capture(&parameters);
        self.apply(&mut parameters);
        sender
            .set_parameters(parameters)
            .map_err(|_| MediaError::Backend)?;
        Ok(Some(defaults))
    }
}

struct VideoSendDefaults {
    max_bitrate: Option<u64>,
    max_framerate: Option<f64>,
    priority: Priority,
    degradation: DegradationPreference,
}

impl VideoSendDefaults {
    fn capture(parameters: &RtpParameters) -> Self {
        let first = parameters.encodings.first();
        Self {
            max_bitrate: first.and_then(|encoding| encoding.max_bitrate),
            max_framerate: first.and_then(|encoding| encoding.max_framerate),
            priority: first.map_or(Priority::Low, |encoding| encoding.priority),
            degradation: parameters.degradation_preference().unwrap_or_default(),
        }
    }

    fn apply(&self, parameters: &mut RtpParameters) {
        if let Some(first) = parameters.encodings.first_mut() {
            first.max_bitrate = self.max_bitrate;
            first.max_framerate = self.max_framerate;
            first.priority = self.priority;
        }
        parameters.set_degradation_preference(self.degradation);
    }

    fn restore(self, sender: &RtpSender) -> Result<(), MediaError> {
        let mut parameters = sender.parameters();
        self.apply(&mut parameters);
        sender
            .set_parameters(parameters)
            .map_err(|_| MediaError::Backend)
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.lifetime.invalidate();
        self.connection.on_ice_candidate(None);
        self.connection.on_connection_state_change(None);
        self.connection.on_track(None);
        self.connection.on_data_channel(None);
        self.connection.close();
    }
}

pub struct MediaEngine {
    local_id: String,
    factory: PeerConnectionFactory,
    peers: BTreeMap<String, Peer>,
    allowed: BTreeSet<String>,
    config: RtcConfiguration,
    events: mpsc::Sender<MediaEvent>,
    microphone: Option<RtcAudioTrack>,
    platform_audio: bool,
    data_only: bool,
    video: Option<RtcVideoTrack>,
    video_profile: Option<VideoSendProfile>,
}

impl MediaEngine {
    pub fn new(local_id: String) -> (Self, mpsc::Receiver<MediaEvent>) {
        let (events, receiver) = mpsc::channel(256);
        (
            Self {
                local_id,
                factory: PeerConnectionFactory::default(),
                peers: BTreeMap::new(),
                allowed: BTreeSet::new(),
                config: RtcConfiguration::default(),
                events,
                microphone: None,
                platform_audio: false,
                data_only: false,
                video: None,
                video_profile: None,
            },
            receiver,
        )
    }

    pub fn configure_ice(
        &mut self,
        servers: Vec<IceServerSettings>,
        relay_only: bool,
    ) -> Result<(), MediaError> {
        self.config.ice_servers = servers
            .into_iter()
            .map(|server| IceServer {
                urls: server.urls,
                username: server.username,
                password: server.credential,
            })
            .collect();
        self.config.ice_transport_type = if relay_only {
            IceTransportsType::Relay
        } else {
            IceTransportsType::All
        };
        for peer in self.peers.values() {
            peer.connection
                .set_configuration(self.config.clone())
                .map_err(|_| MediaError::Backend)?;
        }
        Ok(())
    }

    pub fn authorize_peers(&mut self, peers: impl IntoIterator<Item = String>) {
        self.allowed = peers
            .into_iter()
            .filter(|peer| peer != &self.local_id)
            .collect();
        self.peers.retain(|id, _| self.allowed.contains(id));
    }

    pub fn new_data_only(local_id: String) -> (Self, mpsc::Receiver<MediaEvent>) {
        let (mut engine, events) = Self::new(local_id);
        engine.data_only = true;
        (engine, events)
    }

    pub fn disconnect_peer(&mut self, peer: &str) {
        self.peers.remove(peer);
    }

    pub fn peer_lifetime(&self, peer: &str) -> Option<PeerLifetime> {
        self.peers.get(peer).map(|peer| peer.lifetime.clone())
    }

    pub fn start_microphone(&mut self) -> Result<(), MediaError> {
        if self.microphone.is_some() {
            return Ok(());
        }
        if !self.factory.acquire_platform_adm() {
            return Err(MediaError::Microphone);
        }
        self.platform_audio = true;
        self.factory.set_adm_recording_enabled(true);
        self.factory.set_adm_playout_enabled(true);
        self.microphone = Some(self.factory.create_device_audio_track("zabor-microphone"));
        Ok(())
    }

    pub fn attach_audio_source(&mut self, source: NativeAudioSource) -> Result<(), MediaError> {
        if !self.peers.is_empty() || self.microphone.is_some() {
            return Err(MediaError::Backend);
        }
        self.microphone = Some(self.factory.create_audio_track("zabor-microphone", source));
        self.factory.set_adm_recording_enabled(false);
        self.factory.set_adm_playout_enabled(false);
        Ok(())
    }

    pub fn set_video(&mut self, source: Option<NativeVideoSource>) -> Result<(), MediaError> {
        self.set_video_profile(source, None)
    }

    pub fn set_video_with_quality(
        &mut self,
        source: NativeVideoSource,
        quality: &str,
    ) -> Result<(), MediaError> {
        let profile = VideoSendProfile::parse(quality)?;
        self.set_video_profile(Some(source), Some(profile))
    }

    fn set_video_profile(
        &mut self,
        source: Option<NativeVideoSource>,
        profile: Option<VideoSendProfile>,
    ) -> Result<(), MediaError> {
        if self.data_only {
            return Err(MediaError::Backend);
        }
        if let Some(video) = self.video.take() {
            video.set_enabled(false);
        }
        let mut failed = false;
        for peer in self.peers.values_mut() {
            if let Some(sender) = peer.video_sender.take() {
                if let Some(defaults) = peer.video_defaults.take() {
                    failed |= defaults.restore(&sender).is_err();
                }
                failed |= peer.connection.remove_track(sender).is_err();
            }
        }
        if let Some(source) = source {
            let video = self.factory.create_video_track("zabor-screen", source);
            for peer in self.peers.values_mut() {
                match peer
                    .connection
                    .add_track(video.clone().into(), &["zabor-screen"])
                {
                    Ok(sender) => {
                        if let Some(profile) = profile {
                            match profile.configure(&sender) {
                                Ok(defaults) => peer.video_defaults = defaults,
                                Err(_) => failed = true,
                            }
                        }
                        peer.video_sender = Some(sender);
                    }
                    Err(_) => failed = true,
                }
            }
            self.video = Some(video);
        }
        self.video_profile = self.video.as_ref().and(profile);
        if failed {
            Err(MediaError::Backend)
        } else {
            Ok(())
        }
    }

    pub fn set_muted(&self, muted: bool) {
        if let Some(track) = &self.microphone {
            track.set_enabled(!muted);
        }
    }
    pub fn set_deafened(&self, deafened: bool) {
        if self.platform_audio {
            self.factory.set_adm_playout_enabled(!deafened);
        }
    }

    pub fn recording_devices(&self) -> Vec<(String, String)> {
        (0..self.factory.recording_devices().max(0) as u16)
            .map(|index| {
                (
                    self.factory.recording_device_guid(index),
                    self.factory.recording_device_name(index),
                )
            })
            .collect()
    }

    pub fn playout_devices(&self) -> Vec<(String, String)> {
        (0..self.factory.playout_devices().max(0) as u16)
            .map(|index| {
                (
                    self.factory.playout_device_guid(index),
                    self.factory.playout_device_name(index),
                )
            })
            .collect()
    }

    pub fn select_input(&self, guid: &str) -> Result<(), MediaError> {
        if self.factory.set_recording_device_by_guid(guid) {
            Ok(())
        } else {
            Err(MediaError::Microphone)
        }
    }

    pub fn select_output(&self, guid: &str) -> Result<(), MediaError> {
        if self.factory.set_playout_device_by_guid(guid) {
            Ok(())
        } else {
            Err(MediaError::Backend)
        }
    }

    fn peer(&mut self, id: &str) -> Result<&mut Peer, MediaError> {
        if !self.allowed.contains(id) {
            return Err(MediaError::UnauthorizedPeer);
        }
        if !self.peers.contains_key(id) {
            let connection = self
                .factory
                .create_peer_connection(self.config.clone())
                .map_err(|_| MediaError::Backend)?;
            let lifetime = PeerLifetime::new();
            let live = lifetime.clone();
            let events = self.events.clone();
            let peer_id = id.to_owned();
            connection.on_ice_candidate(Some(Box::new(move |candidate| {
                if !live.is_alive() { return; }
                let payload = json!({"candidate":candidate.candidate(),"sdpMid":candidate.sdp_mid(),"sdpMLineIndex":candidate.sdp_mline_index()}).to_string();
                let _ = events.try_send(MediaEvent::Ice { peer: peer_id.clone(), payload, lifetime: live.clone() });
            })));
            let live = lifetime.clone();
            let events = self.events.clone();
            let peer_id = id.to_owned();
            connection.on_connection_state_change(Some(Box::new(move |state| {
                if !live.is_alive() {
                    return;
                }
                let _ = events.try_send(MediaEvent::Connection {
                    peer: peer_id.clone(),
                    state,
                    lifetime: live.clone(),
                });
            })));
            let live = lifetime.clone();
            let events = self.events.clone();
            let peer_id = id.to_owned();
            connection.on_track(Some(Box::new(move |event| {
                if !live.is_alive() {
                    return;
                }
                let _ = events.try_send(MediaEvent::Track {
                    peer: peer_id.clone(),
                    track: event.track,
                    lifetime: live.clone(),
                });
            })));
            let live = lifetime.clone();
            let events = self.events.clone();
            let peer_id = id.to_owned();
            connection.on_data_channel(Some(Box::new(move |channel| {
                if !live.is_alive() {
                    channel.close();
                    return;
                }
                let _ = events.try_send(MediaEvent::DataChannel {
                    peer: peer_id.clone(),
                    channel,
                    lifetime: live.clone(),
                });
            })));
            let init = RtpTransceiverInit {
                direction: RtpTransceiverDirection::RecvOnly,
                stream_ids: Vec::new(),
                send_encodings: Vec::new(),
            };
            let mut video_sender = None;
            let mut video_defaults = None;
            if !self.data_only {
                if let Some(track) = &self.microphone {
                    connection
                        .add_track(track.clone().into(), &["zabor-voice"])
                        .map_err(|_| MediaError::Backend)?;
                } else {
                    connection
                        .add_transceiver_for_media(MediaType::Audio, init.clone())
                        .map_err(|_| MediaError::Backend)?;
                }
                if let Some(video) = &self.video {
                    let sender = connection
                        .add_track(video.clone().into(), &["zabor-screen"])
                        .map_err(|_| MediaError::Backend)?;
                    if let Some(profile) = self.video_profile {
                        match profile.configure(&sender) {
                            Ok(defaults) => video_defaults = defaults,
                            Err(error) => {
                                lifetime.invalidate();
                                let _ = connection.remove_track(sender);
                                connection.close();
                                return Err(error);
                            }
                        }
                    }
                    video_sender = Some(sender);
                } else {
                    connection
                        .add_transceiver_for_media(MediaType::Video, init)
                        .map_err(|_| MediaError::Backend)?;
                }
            }
            self.peers.insert(
                id.to_owned(),
                Peer {
                    connection,
                    lifetime,
                    epoch: 0,
                    ignored_offer: false,
                    pending_ice: VecDeque::new(),
                    video_sender,
                    video_defaults,
                    needs_offer: false,
                },
            );
        }
        self.peers.get_mut(id).ok_or(MediaError::Backend)
    }

    pub fn create_data_channel(
        &mut self,
        id: &str,
        label: &str,
        init: DataChannelInit,
    ) -> Result<DataChannel, MediaError> {
        self.peer(id)?
            .connection
            .create_data_channel(label, init)
            .map_err(|_| MediaError::Backend)
    }

    pub async fn offer(&mut self, id: &str, ice_restart: bool) -> Result<String, MediaError> {
        let peer = self.peer(id)?;
        if peer.connection.signaling_state() != SignalingState::Stable {
            return Err(MediaError::Backend);
        }
        let mut epoch = NEXT_OFFER_EPOCH.load(Ordering::Acquire);
        peer.epoch = loop {
            let next = epoch
                .checked_add(1)
                .filter(|next| *next <= 9_007_199_254_740_991)
                .ok_or(MediaError::Backend)?;
            match NEXT_OFFER_EPOCH.compare_exchange_weak(
                epoch,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break epoch,
                Err(current) => epoch = current,
            }
        };
        let offer = peer
            .connection
            .create_offer(OfferOptions {
                ice_restart,
                offer_to_receive_audio: None,
                offer_to_receive_video: None,
            })
            .await
            .map_err(|_| MediaError::Backend)?;
        peer.connection
            .set_local_description(offer.clone())
            .await
            .map_err(|_| MediaError::Backend)?;
        Ok(json!({"type":"offer","sdp":offer.to_string(),"epoch":peer.epoch,"allowRemoteControl":false,"remoteControlProtocol":2}).to_string())
    }

    pub async fn offer_when_stable(&mut self, id: &str) -> Result<Option<String>, MediaError> {
        let peer = self.peer(id)?;
        if peer.connection.signaling_state() != SignalingState::Stable {
            peer.needs_offer = true;
            return Ok(None);
        }
        peer.needs_offer = false;
        self.offer(id, false).await.map(Some)
    }

    pub async fn receive_offer(
        &mut self,
        id: &str,
        payload: &str,
    ) -> Result<Option<String>, MediaError> {
        let value = parse_signal(payload)?;
        let polite = self.local_id.as_str() > id;
        let peer = self.peer(id)?;
        if peer.connection.signaling_state() != SignalingState::Stable {
            if !polite {
                peer.ignored_offer = true;
                return Ok(None);
            }
            let rollback = SessionDescription::parse("", SdpType::Rollback)
                .map_err(|_| MediaError::Backend)?;
            peer.connection
                .set_local_description(rollback)
                .await
                .map_err(|_| MediaError::Backend)?;
        }
        peer.ignored_offer = false;
        let description = SessionDescription::parse(
            value["sdp"].as_str().ok_or(MediaError::InvalidSignal)?,
            SdpType::Offer,
        )
        .map_err(|_| MediaError::InvalidSignal)?;
        peer.connection
            .set_remote_description(description)
            .await
            .map_err(|_| MediaError::Backend)?;
        flush_candidates(peer).await?;
        let answer = peer
            .connection
            .create_answer(AnswerOptions::default())
            .await
            .map_err(|_| MediaError::Backend)?;
        peer.connection
            .set_local_description(answer.clone())
            .await
            .map_err(|_| MediaError::Backend)?;
        if peer.needs_offer {
            peer.needs_offer = false;
            let lifetime = peer.lifetime.clone();
            let _ = self.events.try_send(MediaEvent::Renegotiate {
                peer: id.into(),
                lifetime,
            });
        }
        Ok(Some(json!({"type":"answer","sdp":answer.to_string(),"epoch":value.get("epoch"),"allowRemoteControl":false,"remoteControlProtocol":2}).to_string()))
    }

    pub async fn receive_answer(&mut self, id: &str, payload: &str) -> Result<(), MediaError> {
        let value = parse_signal(payload)?;
        let peer = self.peer(id)?;
        if value["epoch"]
            .as_u64()
            .is_some_and(|epoch| epoch != peer.epoch)
            || peer.connection.signaling_state() != SignalingState::HaveLocalOffer
        {
            return Ok(());
        }
        let description = SessionDescription::parse(
            value["sdp"].as_str().ok_or(MediaError::InvalidSignal)?,
            SdpType::Answer,
        )
        .map_err(|_| MediaError::InvalidSignal)?;
        peer.connection
            .set_remote_description(description)
            .await
            .map_err(|_| MediaError::Backend)?;
        flush_candidates(peer).await?;
        peer.ignored_offer = false;
        if peer.needs_offer {
            peer.needs_offer = false;
            let lifetime = peer.lifetime.clone();
            let _ = self.events.try_send(MediaEvent::Renegotiate {
                peer: id.into(),
                lifetime,
            });
        }
        Ok(())
    }

    pub async fn receive_ice(&mut self, id: &str, payload: &str) -> Result<(), MediaError> {
        let value = parse_signal(payload)?;
        let candidate = value["candidate"]
            .as_str()
            .ok_or(MediaError::InvalidSignal)?;
        if candidate.is_empty() {
            return Ok(());
        }
        let index = value["sdpMLineIndex"]
            .as_i64()
            .filter(|index| (0..64).contains(index))
            .ok_or(MediaError::InvalidSignal)?;
        let candidate = IceCandidate::parse(
            value["sdpMid"].as_str().unwrap_or(""),
            index as i32,
            candidate,
        )
        .map_err(|_| MediaError::InvalidSignal)?;
        let peer = self.peer(id)?;
        if peer.connection.current_remote_description().is_none() {
            if peer.pending_ice.len() >= 64 {
                return Err(MediaError::InvalidSignal);
            }
            peer.pending_ice.push_back(candidate);
            Ok(())
        } else {
            let result = peer.connection.add_ice_candidate(candidate).await;
            if result.is_ok() || peer.ignored_offer {
                Ok(())
            } else {
                Err(MediaError::Backend)
            }
        }
    }

    pub fn close(&mut self) {
        self.peers.clear();
        self.allowed.clear();
        if let Some(track) = self.microphone.take() {
            track.set_enabled(false);
        }
        if let Some(video) = self.video.take() {
            video.set_enabled(false);
        }
        self.video_profile = None;
        if self.platform_audio {
            self.factory.set_adm_recording_enabled(false);
            self.factory.stop_recording();
            self.factory.set_adm_playout_enabled(false);
            self.factory.stop_playout();
            self.factory.release_platform_adm();
            self.platform_audio = false;
        }
    }
}

impl Drop for MediaEngine {
    fn drop(&mut self) {
        self.close();
    }
}

async fn flush_candidates(peer: &mut Peer) -> Result<(), MediaError> {
    while let Some(candidate) = peer.pending_ice.pop_front() {
        if peer.connection.add_ice_candidate(candidate).await.is_err() && !peer.ignored_offer {
            return Err(MediaError::Backend);
        }
    }
    Ok(())
}

fn parse_signal(payload: &str) -> Result<Value, MediaError> {
    if payload.len() > 256 * 1024 {
        return Err(MediaError::InvalidSignal);
    }
    let value: Value = serde_json::from_str(payload).map_err(|_| MediaError::InvalidSignal)?;
    if !value.is_object() || value.get("chat").is_some() {
        return Err(MediaError::InvalidSignal);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    #[test]
    fn video_send_profile_rejects_unknown_quality_without_implicit_defaults() {
        for (quality, profile) in [
            ("low", VideoSendProfile::Low),
            ("medium", VideoSendProfile::Medium),
            ("high", VideoSendProfile::High),
            ("camera", VideoSendProfile::Camera),
        ] {
            assert_eq!(VideoSendProfile::parse(quality).unwrap(), profile);
        }
        for quality in ["", "HIGH", "automatic", "1920x1080", "low "] {
            assert!(VideoSendProfile::parse(quality).is_err());
        }
    }

    #[test]
    fn desktop_video_profiles_match_reference_start_limits_and_preserve_rtp_identity() {
        for (profile, bitrate, fps) in [
            (VideoSendProfile::Low, 2_500_000, 30.0),
            (VideoSendProfile::Medium, 3_500_000, 60.0),
            (VideoSendProfile::High, 6_000_000, 60.0),
        ] {
            let encoding = RtpEncodingParameters {
                active: false,
                rid: "original".into(),
                has_ssrc: true,
                ssrc: 42,
                scale_resolution_down_by: Some(2.0),
                scalability_mode: Some("L1T2".into()),
                ..Default::default()
            };
            let untouched = RtpEncodingParameters {
                rid: "second".into(),
                max_bitrate: Some(123_456),
                ..Default::default()
            };
            let mut parameters = RtpParameters::default();
            parameters.encodings = vec![encoding, untouched.clone()];
            parameters.rtcp.cname = "original-rtcp".into();
            profile.apply(&mut parameters);
            assert_eq!(parameters.encodings.len(), 2);
            let first = &parameters.encodings[0];
            assert_eq!(first.max_bitrate, Some(bitrate));
            assert_eq!(first.max_framerate, Some(fps));
            assert_eq!(first.priority, Priority::High);
            assert!(!first.active);
            assert_eq!(first.rid, "original");
            assert!(first.has_ssrc);
            assert_eq!(first.ssrc, 42);
            assert_eq!(first.scale_resolution_down_by, Some(2.0));
            assert_eq!(first.scalability_mode.as_deref(), Some("L1T2"));
            assert_eq!(parameters.rtcp.cname, "original-rtcp");
            assert_eq!(
                format!("{:?}", parameters.encodings[1]),
                format!("{untouched:?}")
            );
            assert_eq!(
                parameters.degradation_preference(),
                Some(DegradationPreference::MaintainFramerate)
            );
            let mut empty = RtpParameters::default();
            profile.apply(&mut empty);
            assert_eq!(empty.encodings.len(), 1);
            assert_eq!(empty.encodings[0].max_bitrate, Some(bitrate));
            assert_eq!(empty.encodings[0].max_framerate, Some(fps));
        }
    }

    #[test]
    fn camera_video_profile_does_not_apply_desktop_encoding_limits() {
        for populated in [false, true] {
            let mut parameters = RtpParameters::default();
            if populated {
                parameters.encodings.push(RtpEncodingParameters {
                    max_bitrate: Some(123_456),
                    max_framerate: Some(15.0),
                    priority: Priority::Medium,
                    ..Default::default()
                });
                parameters.set_degradation_preference(DegradationPreference::Balanced);
            }
            let original = format!("{parameters:?}");
            VideoSendProfile::Camera.apply(&mut parameters);
            assert_eq!(format!("{parameters:?}"), original);
        }
    }

    #[test]
    fn video_profile_restores_original_limits_without_replacing_rtp_encoding_identity() {
        for populated in [false, true] {
            let mut parameters = RtpParameters::default();
            if populated {
                parameters.encodings.push(RtpEncodingParameters {
                    max_bitrate: Some(123_456),
                    max_framerate: Some(15.0),
                    priority: Priority::Medium,
                    rid: "original".into(),
                    has_ssrc: true,
                    ssrc: 42,
                    ..Default::default()
                });
                parameters.set_degradation_preference(DegradationPreference::MaintainResolution);
            }
            let defaults = VideoSendDefaults::capture(&parameters);
            VideoSendProfile::High.apply(&mut parameters);
            parameters.encodings[0].ssrc = 43;
            defaults.apply(&mut parameters);
            let first = &parameters.encodings[0];
            assert_eq!(first.max_bitrate, populated.then_some(123_456));
            assert_eq!(first.max_framerate, populated.then_some(15.0));
            assert_eq!(
                first.priority,
                if populated {
                    Priority::Medium
                } else {
                    Priority::Low
                }
            );
            assert_eq!(first.ssrc, 43);
            assert_eq!(first.rid, if populated { "original" } else { "" });
            assert_eq!(
                parameters.degradation_preference(),
                Some(if populated {
                    DegradationPreference::MaintainResolution
                } else {
                    DegradationPreference::Balanced
                })
            );
        }
    }

    #[tokio::test]
    async fn native_video_senders_keep_quality_across_late_peers_and_source_replacement() {
        use libwebrtc::video_source::VideoResolution;
        for preexisting_peer in [false, true] {
            for (quality, bitrate, fps) in [
                ("low", Some(2_500_000), Some(30.0)),
                ("medium", Some(3_500_000), Some(60.0)),
                ("high", Some(6_000_000), Some(60.0)),
                ("camera", None, None),
            ] {
                let (mut engine, _) = MediaEngine::new("alice".into());
                engine.authorize_peers(["bob".into(), "carol".into()]);
                if preexisting_peer {
                    engine.peer("bob").unwrap();
                }
                let source = NativeVideoSource::new(
                    VideoResolution {
                        width: 160,
                        height: 96,
                    },
                    quality != "camera",
                );
                engine
                    .set_video_with_quality(source.clone(), quality)
                    .unwrap();
                engine.peer("bob").unwrap();
                engine.peer("carol").unwrap();
                let lifetime = engine.peer_lifetime("bob").unwrap();
                for id in ["bob", "carol"] {
                    let parameters = engine.peers[id].video_sender.as_ref().unwrap().parameters();
                    assert_eq!(parameters.encodings[0].max_bitrate, bitrate);
                    assert_eq!(parameters.encodings[0].max_framerate, fps);
                    if quality != "camera" {
                        assert_eq!(parameters.encodings[0].priority, Priority::High);
                        assert_eq!(
                            parameters.degradation_preference(),
                            Some(DegradationPreference::MaintainFramerate)
                        );
                    }
                }
                let original = engine.video.as_ref().unwrap().clone();
                assert!(engine
                    .set_video_with_quality(source.clone(), "invalid")
                    .is_err());
                assert!(original.enabled());
                assert!(engine.peer_lifetime("bob").unwrap().is_same(&lifetime));
                engine
                    .set_video_with_quality(source.clone(), "high")
                    .unwrap();
                assert!(!original.enabled());
                assert!(lifetime.is_alive());
                assert!(engine.peer_lifetime("bob").unwrap().is_same(&lifetime));
                for peer in engine.peers.values() {
                    let parameters = peer.video_sender.as_ref().unwrap().parameters();
                    assert_eq!(parameters.encodings[0].max_bitrate, Some(6_000_000));
                    assert_eq!(parameters.encodings[0].max_framerate, Some(60.0));
                }
                engine
                    .set_video_with_quality(source.clone(), "camera")
                    .unwrap();
                for peer in engine.peers.values() {
                    let parameters = peer.video_sender.as_ref().unwrap().parameters();
                    assert!(parameters.encodings[0].max_bitrate.is_none());
                    assert!(parameters.encodings[0].max_framerate.is_none());
                    assert_eq!(parameters.encodings[0].priority, Priority::Low);
                }
                engine
                    .set_video_with_quality(source.clone(), "low")
                    .unwrap();
                engine.set_video(None).unwrap();
                assert!(engine.video_profile.is_none());
                assert!(engine
                    .peers
                    .values()
                    .all(|peer| peer.video_sender.is_none()));
                engine.set_video(Some(source)).unwrap();
                assert!(engine.video_profile.is_none());
                for peer in engine.peers.values() {
                    let parameters = peer.video_sender.as_ref().unwrap().parameters();
                    assert!(parameters.encodings[0].max_bitrate.is_none());
                    assert!(parameters.encodings[0].max_framerate.is_none());
                }
                engine.close();
                assert!(engine.video_profile.is_none());
                assert!(!lifetime.is_alive());
            }
        }
    }

    #[test]
    fn peer_commit_linearizes_invalidation_and_rejects_every_later_callback() {
        let lifetime = PeerLifetime::new();
        assert!(lifetime.is_same(&lifetime.clone()));
        assert!(!lifetime.is_same(&PeerLifetime::new()));
        let (entered, waiting) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel();
        let (started, starting) = std::sync::mpsc::channel();
        let (done, finished) = std::sync::mpsc::channel();
        let committed = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let lifetime = &lifetime;
            let committed = &committed;
            let commit = scope.spawn(move || {
                lifetime.commit(|| {
                    entered.send(()).unwrap();
                    held.recv_timeout(Duration::from_secs(3)).unwrap();
                    assert!(lifetime.is_alive());
                    committed.store(1, Ordering::Release);
                    42
                })
            });
            waiting.recv_timeout(Duration::from_secs(1)).unwrap();
            let invalidate = scope.spawn(move || {
                started.send(()).unwrap();
                lifetime.invalidate();
                assert_eq!(committed.load(Ordering::Acquire), 1);
                done.send(()).unwrap();
            });
            starting.recv_timeout(Duration::from_secs(1)).unwrap();
            assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
            release.send(()).unwrap();
            assert_eq!(commit.join().unwrap(), Some(42));
            finished.recv_timeout(Duration::from_secs(1)).unwrap();
            invalidate.join().unwrap();
        });
        assert!(!lifetime.is_alive());
        assert!(lifetime
            .commit(|| panic!("Invalidated peer commit ran"))
            .is_none());
        assert!(lifetime.0.stop.is_cancelled());
        println!("peer acceptance invalidation strict_linearization=true original_identity=true physical_audio=false");
    }

    #[test]
    fn peer_commit_capture_drop_is_outside_gate_and_poison_never_reopens_admission() {
        struct Probe {
            lifetime: PeerLifetime,
            drops: Arc<AtomicUsize>,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                let _ = self.lifetime.commit(|| 43);
                self.drops.fetch_add(1, Ordering::AcqRel);
            }
        }
        for mode in ["live", "invalid", "panic", "poison"] {
            let lifetime = PeerLifetime::new();
            if mode == "invalid" {
                lifetime.invalidate();
            }
            if mode == "poison" {
                assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || lifetime.commit(|| panic!("Expected peer gate poison"))
                ))
                .is_err());
            }
            let drops = Arc::new(AtomicUsize::new(0));
            let probe = Probe {
                lifetime: lifetime.clone(),
                drops: drops.clone(),
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                lifetime.commit(move || {
                    let _ = &probe;
                    assert_ne!(mode, "panic", "Expected peer commit callback unwind");
                    42
                })
            }));
            assert_eq!(result.is_err(), mode == "panic");
            if let Ok(result) = result {
                assert_eq!(result, (mode == "live").then_some(42));
            }
            assert_eq!(drops.load(Ordering::Acquire), 1);
            if matches!(mode, "poison" | "panic") {
                assert!(lifetime.is_alive());
                assert!(lifetime
                    .commit(|| panic!("Poisoned peer admission reopened"))
                    .is_none());
            }
            lifetime.invalidate();
            assert!(!lifetime.is_alive());
            assert!(lifetime.0.stop.is_cancelled());
            assert!(lifetime
                .commit(|| panic!("Invalidation reopened peer admission"))
                .is_none());
            println!("peer acceptance capture={mode} drop_gate_reentry=true poison_fail_closed=true physical_audio=false");
        }
    }

    #[test]
    fn peer_invalidation_cancels_after_gate_release_even_with_poisoned_admission() {
        use std::future::Future;
        use std::task::{Context, Poll, Wake, Waker};
        struct Observer {
            lifetime: PeerLifetime,
            wakes: AtomicUsize,
        }
        impl Wake for Observer {
            fn wake(self: Arc<Self>) {
                self.wake_by_ref();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                let _gate = match self.lifetime.0.commit.try_lock() {
                    Ok(gate) => gate,
                    Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
                    Err(std::sync::TryLockError::WouldBlock) => {
                        panic!("Peer cancellation ran under acceptance gate")
                    }
                };
                assert!(!self.lifetime.is_alive());
                self.wakes.fetch_add(1, Ordering::AcqRel);
            }
        }
        for poison in [false, true] {
            let lifetime = PeerLifetime::new();
            if poison {
                assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || lifetime.commit(|| panic!("Expected peer admission poison"))
                ))
                .is_err());
            }
            let observer = Arc::new(Observer {
                lifetime: lifetime.clone(),
                wakes: AtomicUsize::new(0),
            });
            let waker = Waker::from(observer.clone());
            let mut context = Context::from_waker(&waker);
            let mut cancelled = Box::pin(lifetime.cancelled());
            assert_eq!(cancelled.as_mut().poll(&mut context), Poll::Pending);
            lifetime.invalidate();
            assert_eq!(observer.wakes.load(Ordering::Acquire), 1);
            assert_eq!(cancelled.as_mut().poll(&mut context), Poll::Ready(()));
            lifetime.invalidate();
            assert_eq!(observer.wakes.load(Ordering::Acquire), 1);
            println!("peer acceptance cancellation poison={poison} wake_after_gate_release=true idempotent=true physical_audio=false");
        }
    }
}
