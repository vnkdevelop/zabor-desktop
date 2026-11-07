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
    Arc,
};
use tokio::sync::mpsc;

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
pub struct PeerLifetime(Arc<AtomicBool>);

impl PeerLifetime {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
    pub fn is_alive(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn invalidate(&self) {
        self.0.store(false, Ordering::Release);
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
        match self {
            Self::Ice { lifetime, .. }
            | Self::Connection { lifetime, .. }
            | Self::Track { lifetime, .. }
            | Self::DataChannel { lifetime, .. }
            | Self::Renegotiate { lifetime, .. } => lifetime.is_alive(),
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
    needs_offer: bool,
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
        if self.data_only {
            return Err(MediaError::Backend);
        }
        if let Some(video) = self.video.take() {
            video.set_enabled(false);
        }
        let mut failed = false;
        for peer in self.peers.values_mut() {
            if let Some(sender) = peer.video_sender.take() {
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
                    Ok(sender) => peer.video_sender = Some(sender),
                    Err(_) => failed = true,
                }
            }
            self.video = Some(video);
        }
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
                    video_sender = Some(
                        connection
                            .add_track(video.clone().into(), &["zabor-screen"])
                            .map_err(|_| MediaError::Backend)?,
                    );
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
