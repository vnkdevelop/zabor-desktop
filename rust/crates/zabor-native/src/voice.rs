use crate::audio_mix::AudioControls;
use crate::audio_playback::AudioPlayback;
#[cfg(windows)]
use crate::audio_runtime::{SharedAudio, VoiceLease};
use crate::auth::User;
#[cfg(windows)]
use crate::camera::CameraCapture;
use crate::hub::HubConnection;
use crate::media::PeerLifetime;
use crate::media::{IceServerSettings, MediaEngine, MediaError, MediaEvent};
#[cfg(windows)]
use crate::native_audio::NativeAudio;
use crate::screen::{capture_format, ScreenCapture, ScreenSource};
use libwebrtc::audio_track::RtcAudioTrack;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Deserialize)]
pub struct ChannelMembership {
    pub users: Vec<User>,
}

enum ShareCapture {
    Desktop(ScreenCapture),
    #[cfg(windows)]
    Camera(CameraCapture),
}

impl ShareCapture {
    fn source(&self) -> libwebrtc::video_source::native::NativeVideoSource {
        match self {
            Self::Desktop(capture) => capture.source.clone(),
            #[cfg(windows)]
            Self::Camera(capture) => capture.source.clone(),
        }
    }

    fn is_live(&self) -> bool {
        match self {
            Self::Desktop(capture) => capture.is_live(),
            #[cfg(windows)]
            Self::Camera(capture) => capture.is_live(),
        }
    }

    fn failed(&self) -> bool {
        match self {
            Self::Desktop(capture) => capture.failed(),
            #[cfg(windows)]
            Self::Camera(capture) => capture.failed(),
        }
    }
}

struct ScreenShare {
    capture: ShareCapture,
    quality: String,
}

pub struct VoiceSession {
    engine: MediaEngine,
    local_id: String,
    peers: BTreeSet<String>,
    pub channel: Option<String>,
    pub call_peer: Option<String>,
    relay_only: bool,
    capture: Option<ScreenShare>,
    controls: Arc<AudioControls>,
    #[cfg(windows)]
    audio_runtime: Arc<SharedAudio<NativeAudio>>,
    #[cfg(windows)]
    audio: Option<VoiceLease<NativeAudio>>,
    playback: Option<AudioPlayback>,
}

impl VoiceSession {
    pub async fn new(
        hub: &HubConnection,
        user: &User,
        relay_only: bool,
        controls: Arc<AudioControls>,
        #[cfg(windows)] audio_runtime: Arc<SharedAudio<NativeAudio>>,
    ) -> Result<(Self, mpsc::Receiver<MediaEvent>), MediaError> {
        let local_id = user.id.clone();
        let (mut engine, events) = MediaEngine::new(local_id.clone());
        let settings = hub
            .invoke("GetIceServers", vec![], Duration::from_secs(5))
            .await
            .map_err(|_| MediaError::Backend)?;
        let servers: Vec<IceServerSettings> =
            serde_json::from_value(settings["iceServers"].clone())
                .map_err(|_| MediaError::Backend)?;
        if relay_only
            && !servers.iter().any(|server| {
                server
                    .urls
                    .iter()
                    .any(|url| url.starts_with("turn:") || url.starts_with("turns:"))
            })
        {
            return Err(MediaError::Backend);
        }
        engine.configure_ice(servers, relay_only)?;
        Ok((
            Self {
                engine,
                local_id,
                peers: BTreeSet::new(),
                channel: None,
                call_peer: None,
                relay_only,
                capture: None,
                controls,
                #[cfg(windows)]
                audio_runtime,
                #[cfg(windows)]
                audio: None,
                playback: None,
            },
            events,
        ))
    }

    pub async fn join(
        &mut self,
        hub: &HubConnection,
        channel: &str,
    ) -> Result<Vec<User>, MediaError> {
        self.start_audio().await?;
        self.engine.set_muted(self.controls.effectively_muted());
        self.engine
            .set_deafened(self.controls.effectively_deafened());
        let value = hub
            .invoke(
                "JoinChannel",
                vec![json!({"channelId":channel})],
                Duration::from_secs(20),
            )
            .await
            .map_err(|_| MediaError::Backend)?;
        if value.is_null() {
            return Err(MediaError::ChannelFull);
        }
        let membership: ChannelMembership =
            serde_json::from_value(value).map_err(|_| MediaError::Backend)?;
        self.channel = Some(channel.to_owned());
        self.call_peer = None;
        let previous = self.peers.clone();
        self.peers = membership
            .users
            .iter()
            .filter(|user| user.id != self.local_id)
            .map(|user| user.id.clone())
            .collect();
        self.engine.authorize_peers(self.peers.iter().cloned());
        #[cfg(windows)]
        if let Some(audio) = &self.audio {
            if !audio.activate() {
                return Err(MediaError::Microphone);
            }
        }
        for id in self.peers.iter().filter(|id| !previous.contains(*id)) {
            let offer = self.engine.offer(id, false).await?;
            hub.send("SendWebRTCOffer", vec![json!(id), json!(offer)])
                .await
                .map_err(|_| MediaError::Backend)?;
        }
        Ok(membership.users)
    }

    pub async fn leave(&mut self, hub: &HubConnection) {
        let was_call = self.call_peer.take().is_some();
        let was_sharing = self.capture.is_some();
        self.capture = None;
        self.stop_audio();
        self.engine.close();
        self.peers.clear();
        self.channel = None;
        if was_sharing {
            let _ = hub.send("StopStream", Vec::new()).await;
        }
        let _ = hub
            .invoke(
                if was_call { "EndCall" } else { "LeaveChannel" },
                vec![],
                Duration::from_secs(5),
            )
            .await;
    }

    pub async fn rejoin(&mut self, hub: &HubConnection) -> Result<Vec<User>, MediaError> {
        let channel = self.channel.clone().ok_or(MediaError::Backend)?;
        let settings = hub
            .invoke("GetIceServers", Vec::new(), Duration::from_secs(5))
            .await
            .map_err(|_| MediaError::Backend)?;
        let servers = serde_json::from_value(settings["iceServers"].clone())
            .map_err(|_| MediaError::Backend)?;
        self.engine.configure_ice(servers, self.relay_only)?;
        self.peers.clear();
        self.engine.authorize_peers(Vec::<String>::new());
        self.join(hub, &channel).await
    }

    pub async fn prepare_call(&mut self, peer: &str) -> Result<(), MediaError> {
        self.start_audio().await?;
        self.engine.set_muted(self.controls.effectively_muted());
        self.engine
            .set_deafened(self.controls.effectively_deafened());
        self.channel = None;
        self.call_peer = Some(peer.into());
        self.peers = [peer.to_owned()].into_iter().collect();
        self.engine.authorize_peers(self.peers.iter().cloned());
        #[cfg(windows)]
        if let Some(audio) = &self.audio {
            if !audio.activate() {
                return Err(MediaError::Microphone);
            }
        }
        Ok(())
    }

    pub async fn connect_call(
        &mut self,
        hub: &HubConnection,
        peer: &str,
    ) -> Result<(), MediaError> {
        if self.call_peer.as_deref() != Some(peer) {
            return Err(MediaError::UnauthorizedPeer);
        }
        let offer = self.engine.offer(peer, false).await?;
        hub.send("SendWebRTCOffer", vec![json!(peer), json!(offer)])
            .await
            .map_err(|_| MediaError::Backend)
    }

    pub async fn renegotiate(&mut self, hub: &HubConnection, peer: &str) -> Result<(), MediaError> {
        if !self.peers.contains(peer) {
            return Ok(());
        }
        if let Some(offer) = self.engine.offer_when_stable(peer).await? {
            hub.send("SendWebRTCOffer", vec![json!(peer), json!(offer)])
                .await
                .map_err(|_| MediaError::Backend)?;
        }
        Ok(())
    }

    pub async fn resume_call(
        &mut self,
        hub: &HubConnection,
        peer: &str,
        servers: Vec<IceServerSettings>,
    ) -> Result<(), MediaError> {
        if self.call_peer.as_deref() != Some(peer) || !self.peers.contains(peer) {
            return Err(MediaError::UnauthorizedPeer);
        }
        if self.relay_only
            && !servers.iter().any(|server| {
                server
                    .urls
                    .iter()
                    .any(|url| url.starts_with("turn:") || url.starts_with("turns:"))
            })
        {
            return Err(MediaError::Backend);
        }
        self.engine.configure_ice(servers, self.relay_only)?;
        self.engine.disconnect_peer(peer);
        if let Some(playback) = self.playback.as_mut() {
            playback.retain_alive();
        }
        let offer = self.engine.offer(peer, true).await?;
        hub.send("SendWebRTCOffer", vec![json!(peer), json!(offer)])
            .await
            .map_err(|_| MediaError::Backend)
    }

    pub async fn start_share(
        &mut self,
        hub: &HubConnection,
        source: ScreenSource,
        quality: &str,
    ) -> Result<(), MediaError> {
        let (width, height, fps) = capture_format(quality)?;
        let capture = tokio::task::spawn_blocking(move || {
            ScreenCapture::start(source, width, height, fps, true)
        })
        .await
        .map_err(|_| MediaError::Backend)??;
        self.announce_share(hub, ShareCapture::Desktop(capture), quality)
            .await
    }

    #[cfg(windows)]
    pub async fn start_camera_share(
        &mut self,
        hub: &HubConnection,
        device: String,
    ) -> Result<(), MediaError> {
        let capture = CameraCapture::start(device, self.audio_runtime.cleanup_tracker()).await?;
        self.announce_share(hub, ShareCapture::Camera(capture), "camera")
            .await
    }

    async fn announce_share(
        &mut self,
        hub: &HubConnection,
        capture: ShareCapture,
        quality: &str,
    ) -> Result<(), MediaError> {
        let started = hub
            .invoke("StartStream", vec![json!(quality)], Duration::from_secs(10))
            .await
            .map_err(|_| MediaError::Backend)?;
        if started != true {
            return Err(MediaError::Backend);
        }
        if let Err(error) = self
            .engine
            .set_video_with_quality(capture.source(), quality)
        {
            let _ = self.engine.set_video(None);
            let _ = hub.send("StopStream", Vec::new()).await;
            return Err(error);
        }
        self.capture = Some(ScreenShare {
            capture,
            quality: quality.to_owned(),
        });
        for peer in self.peers.clone() {
            self.renegotiate(hub, &peer).await?;
        }
        Ok(())
    }

    pub async fn restore_share(&self, hub: &HubConnection) -> Result<bool, MediaError> {
        let Some(share) = self.capture.as_ref().filter(|share| {
            share.capture.is_live()
                && self.engine.has_live_video()
                && (self.channel.is_some() || self.call_peer.is_some())
        }) else {
            return Ok(false);
        };
        let started = hub
            .invoke(
                "StartStream",
                vec![json!(share.quality)],
                Duration::from_secs(10),
            )
            .await
            .map_err(|_| MediaError::Backend)?;
        if started != true {
            return Err(MediaError::Backend);
        }
        Ok(share.capture.is_live() && self.engine.has_live_video())
    }

    pub async fn stop_share(&mut self, hub: &HubConnection) {
        self.capture = None;
        let _ = self.engine.set_video(None);
        let _ = hub.send("StopStream", Vec::new()).await;
        for peer in self.peers.clone() {
            let _ = self.renegotiate(hub, &peer).await;
        }
    }

    pub fn share_failed(&self) -> bool {
        self.capture
            .as_ref()
            .is_some_and(|share| share.capture.failed())
    }
    pub fn is_sharing(&self) -> bool {
        self.capture.is_some()
    }

    pub fn has_peer(&self, peer: &str) -> bool {
        self.peers.contains(peer)
    }

    async fn start_audio(&mut self) -> Result<(), MediaError> {
        #[cfg(windows)]
        {
            if self.audio.is_some() {
                return Ok(());
            }
            let audio = self.audio_runtime.voice().await?;
            let device = audio.device().ok_or(MediaError::Microphone)?;
            self.engine.attach_audio_source(device.source.clone())?;
            self.playback = Some(AudioPlayback::with_cleanup(
                device.mixer.clone(),
                self.audio_runtime.cleanup_tracker(),
            ));
            self.audio = Some(audio);
            Ok(())
        }
        #[cfg(not(windows))]
        Err(MediaError::Microphone)
    }

    fn stop_audio(&mut self) {
        #[cfg(windows)]
        if let Some(audio) = &self.audio {
            audio.clear_network_status();
        }
        #[cfg(not(windows))]
        {
            self.controls.speaking.store(false, Ordering::Release);
            self.controls
                .mic_db
                .store((-100.0_f32).to_bits(), Ordering::Release);
        }
        self.playback = None;
        #[cfg(windows)]
        {
            self.audio = None;
        }
    }

    pub fn receive_audio(
        &mut self,
        peer: &str,
        track: RtcAudioTrack,
        lifetime: PeerLifetime,
    ) -> Result<(), MediaError> {
        if !self.peers.contains(peer) {
            return Err(MediaError::UnauthorizedPeer);
        }
        self.playback
            .as_mut()
            .ok_or(MediaError::Backend)?
            .attach(peer, track, lifetime, false)
    }

    pub fn audio_failed(&mut self) -> bool {
        if let Some(playback) = &mut self.playback {
            playback.retain_alive();
        }
        let failed = self.controls.processor_failed.load(Ordering::Acquire);
        #[cfg(windows)]
        let failed = failed || self.audio.as_ref().is_some_and(VoiceLease::failed);
        failed
    }

    pub async fn mute(
        &mut self,
        hub: &HubConnection,
        muted: bool,
        deafened: bool,
    ) -> Result<(), MediaError> {
        self.engine.set_muted(self.controls.effectively_muted());
        self.engine
            .set_deafened(self.controls.effectively_deafened());
        hub.send(
            "UpdateUserState",
            vec![json!({"userId":self.local_id,"isMuted":muted,"isDeafened":deafened})],
        )
        .await
        .map_err(|_| MediaError::Backend)
    }

    pub async fn handle(
        &mut self,
        hub: &HubConnection,
        target: &str,
        args: &[Value],
    ) -> Result<(), MediaError> {
        let peer = args.first().and_then(Value::as_str);
        let payload = args.get(1).and_then(Value::as_str);
        match target {
            "UserStateChanged" | "UserUpdated" => {
                if let Some(user) = args.first() {
                    if user["userId"]
                        .as_str()
                        .or_else(|| user["UserId"].as_str())
                        .or_else(|| user["id"].as_str())
                        == Some(self.local_id.as_str())
                    {
                        let server_muted = user["isServerMuted"]
                            .as_bool()
                            .or_else(|| user["IsServerMuted"].as_bool())
                            .unwrap_or_else(|| self.controls.server_state().0);
                        let server_deafened = user["isServerDeafened"]
                            .as_bool()
                            .or_else(|| user["IsServerDeafened"].as_bool())
                            .unwrap_or_else(|| self.controls.server_state().1);
                        self.controls
                            .set_server_state(server_muted, server_deafened);
                        self.engine.set_muted(self.controls.effectively_muted());
                        self.engine
                            .set_deafened(self.controls.effectively_deafened());
                    }
                }
            }
            "ChannelDeleted" if self.channel.is_some() && peer == self.channel.as_deref() => {
                self.capture = None;
                self.stop_audio();
                self.engine.close();
                self.peers.clear();
                self.channel = None;
            }
            "ReceiveWebRTCOffer" | "ReceiveWebRTCAnswer" | "ReceiveIceCandidate" => {
                let (Some(peer), Some(payload)) = (peer, payload) else {
                    return Err(MediaError::InvalidSignal);
                };
                if payload.starts_with("zabor-chat:") {
                    return Ok(());
                }
                if !self.peers.contains(peer) {
                    return Ok(());
                }
                match target {
                    "ReceiveWebRTCOffer" => {
                        if let Some(answer) = self.engine.receive_offer(peer, payload).await? {
                            hub.send("SendWebRTCAnswer", vec![json!(peer), json!(answer)])
                                .await
                                .map_err(|_| MediaError::Backend)?;
                        }
                    }
                    "ReceiveWebRTCAnswer" => self.engine.receive_answer(peer, payload).await?,
                    _ => self.engine.receive_ice(peer, payload).await?,
                }
            }
            "UserJoinedChannel" => {
                let user: User = serde_json::from_value(args.first().cloned().unwrap_or_default())
                    .map_err(|_| MediaError::InvalidSignal)?;
                if self.channel.is_some()
                    && args.get(1).and_then(Value::as_str) == self.channel.as_deref()
                    && !user.id.is_empty()
                    && user.id != self.local_id
                    && self.peers.insert(user.id.clone())
                {
                    self.engine.authorize_peers(self.peers.iter().cloned());
                    let offer = self.engine.offer(&user.id, false).await?;
                    hub.send("SendWebRTCOffer", vec![json!(user.id), json!(offer)])
                        .await
                        .map_err(|_| MediaError::Backend)?;
                }
            }
            "UserLeftChannel" | "UserLeft" | "UserDeleted" => {
                if let Some(peer) = peer {
                    if target != "UserLeftChannel"
                        || (self.channel.is_some()
                            && args.get(1).and_then(Value::as_str) == self.channel.as_deref())
                    {
                        self.peers.remove(peer);
                        self.engine.authorize_peers(self.peers.iter().cloned());
                    }
                }
            }
            "SyncFullChannelState" => {
                if let Some(channel) = &self.channel {
                    let state: std::collections::BTreeMap<String, Vec<User>> =
                        serde_json::from_value(args.first().cloned().unwrap_or_default())
                            .map_err(|_| MediaError::InvalidSignal)?;
                    let peers: BTreeSet<String> = state
                        .get(channel)
                        .into_iter()
                        .flatten()
                        .filter(|user| !user.id.is_empty() && user.id != self.local_id)
                        .map(|user| user.id.clone())
                        .collect();
                    let added: Vec<String> = peers.difference(&self.peers).cloned().collect();
                    self.peers = peers;
                    self.engine.authorize_peers(self.peers.iter().cloned());
                    for peer in added {
                        self.renegotiate(hub, &peer).await?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Drop for VoiceSession {
    fn drop(&mut self) {
        self.stop_audio();
    }
}

#[cfg(all(test, windows))]
#[path = "voice/stream_tests.rs"]
mod stream_tests;

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::audio_settings::AudioSettings;
    use std::time::Instant;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn unstarted_voice_stop_and_drop_cannot_clear_fresh_owner_network_controls() {
        let controls = Arc::new(AudioControls::new(AudioSettings::default()));
        let shared = SharedAudio::new(controls.clone(), CancellationToken::new(), |_, _, _| {
            Box::pin(async { panic!("Voice cleanup fixture must not open physical audio") })
        });
        let monitor = shared.monitor(true);
        let generation = shared.demand.reserve().unwrap();
        assert!(shared.demand.activate(generation));
        controls.speaking.store(true, Ordering::Release);
        controls
            .mic_db
            .store((-17.0_f32).to_bits(), Ordering::Release);
        let epoch = controls.capture_epoch.load(Ordering::Acquire);
        let snapshot = shared.mic_test.snapshot(Instant::now());
        let (engine, _events) = MediaEngine::new_data_only("old-owner".into());
        let mut session = VoiceSession {
            engine,
            local_id: "old-owner".into(),
            peers: BTreeSet::new(),
            channel: None,
            call_peer: None,
            relay_only: false,
            capture: None,
            controls: controls.clone(),
            audio_runtime: shared.clone(),
            audio: None,
            playback: None,
        };
        session.stop_audio();
        drop(session);
        assert_eq!(shared.demand.active(), generation);
        assert!(shared.demand.is_reserved(generation));
        assert_eq!(controls.capture_epoch.load(Ordering::Acquire), epoch);
        assert!(controls.speaking.load(Ordering::Acquire));
        assert_eq!(
            f32::from_bits(controls.mic_db.load(Ordering::Acquire)),
            -17.0
        );
        assert!(monitor.is_current());
        assert_eq!(shared.mic_test.snapshot(Instant::now()), snapshot);
        println!("voice session cleanup audio_lease=false actual_stop_and_drop=true current_generation=true controls_unchanged=true monitor_unchanged=true physical_audio=false");
    }
}
