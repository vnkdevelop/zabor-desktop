use super::*;
use libwebrtc::peer_connection::IceGatheringState;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryMode {
    P2p,
    Sfu,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamDelivery {
    pub streamer: String,
    pub mode: DeliveryMode,
    pub epoch: u64,
}

impl StreamDelivery {
    pub fn parse(value: &Value) -> Result<Self, MediaError> {
        let streamer = value
            .get("streamerId")
            .or_else(|| value.get("StreamerId"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .ok_or(MediaError::InvalidSignal)?
            .to_owned();
        let epoch = value
            .get("epoch")
            .or_else(|| value.get("Epoch"))
            .and_then(Value::as_u64)
            .filter(|epoch| *epoch <= 9_007_199_254_740_991)
            .ok_or(MediaError::InvalidSignal)?;
        let mode = if value
            .get("mode")
            .or_else(|| value.get("Mode"))
            .and_then(Value::as_str)
            == Some("sfu")
        {
            DeliveryMode::Sfu
        } else {
            DeliveryMode::P2p
        };
        Ok(Self {
            streamer,
            mode,
            epoch,
        })
    }
}

#[derive(Default)]
pub(super) struct SfuMedia {
    publisher: Option<SfuPeer>,
    pub subscribers: BTreeMap<String, SfuPeer>,
    epoch: u64,
    active: bool,
    fallback: BTreeSet<String>,
    ready: BTreeSet<String>,
    close_at: Option<Instant>,
}

pub(super) struct SfuPeer {
    connection: PeerConnection,
    lifetime: PeerLifetime,
    epoch: u64,
    video_sender: Option<RtpSender>,
    audio_sender: Option<RtpSender>,
    candidates: Arc<Mutex<Vec<(String, i32, String)>>>,
    ready: bool,
    failure_reported: bool,
    deadline: Instant,
    preview_close: Option<Instant>,
    adaptive: AdaptiveVideo,
}

impl Drop for SfuPeer {
    fn drop(&mut self) {
        self.lifetime.invalidate();
        self.connection.on_ice_candidate(None);
        self.connection.on_connection_state_change(None);
        self.connection.on_track(None);
        self.connection.close();
    }
}

impl SfuPeer {
    fn new(
        engine: &MediaEngine,
        peer: &str,
        epoch: u64,
        publisher: bool,
    ) -> Result<Self, MediaError> {
        let connection = engine
            .factory
            .create_peer_connection(engine.config.clone())
            .map_err(|_| MediaError::Backend)?;
        let lifetime = PeerLifetime::new();
        let candidates = Arc::new(Mutex::new(Vec::new()));
        let mut result = Self {
            connection,
            lifetime,
            epoch,
            video_sender: None,
            audio_sender: None,
            candidates,
            ready: false,
            failure_reported: false,
            deadline: Instant::now() + Duration::from_millis(if publisher { 6500 } else { 10000 }),
            preview_close: None,
            adaptive: AdaptiveVideo::default(),
        };
        let live = result.lifetime.clone();
        let ice = result.candidates.clone();
        result
            .connection
            .on_ice_candidate(Some(Box::new(move |candidate| {
                if live.is_alive() {
                    if let Ok(mut ice) = ice.lock() {
                        if ice.len() < 64 {
                            ice.push((
                                candidate.sdp_mid(),
                                candidate.sdp_mline_index(),
                                candidate.candidate(),
                            ));
                        }
                    }
                }
            })));
        let live = result.lifetime.clone();
        let events = engine.events.clone();
        let id = peer.to_owned();
        result
            .connection
            .on_connection_state_change(Some(Box::new(move |state| {
                if live.is_alive() {
                    let _ = events.try_send(MediaEvent::SfuConnection {
                        peer: id.clone(),
                        epoch,
                        publisher,
                        state,
                        lifetime: live.clone(),
                    });
                }
            })));
        if publisher {
            let video = engine
                .video
                .as_ref()
                .filter(|_| engine.has_live_video())
                .ok_or(MediaError::Backend)?;
            let sender = result
                .connection
                .add_transceiver(
                    video.clone().into(),
                    RtpTransceiverInit {
                        direction: RtpTransceiverDirection::SendOnly,
                        stream_ids: vec!["zabor-screen".into()],
                        send_encodings: Vec::new(),
                    },
                )
                .map_err(|_| MediaError::Backend)?
                .sender();
            if let Some(quality) = engine.video_quality() {
                if let Some(profile) =
                    VideoProfile::for_quality(quality, 0, ViewerState::Watching, false)?
                {
                    let mut parameters = sender.parameters();
                    profile.apply(&mut parameters);
                    sender
                        .set_parameters(parameters)
                        .map_err(|_| MediaError::Backend)?;
                    result.adaptive.applied = Some(profile);
                }
            }
            result.video_sender = Some(sender);
            if let Some(audio) = engine
                .stream_audio
                .as_ref()
                .filter(|_| engine.has_live_stream_audio())
            {
                result.audio_sender = Some(
                    result
                        .connection
                        .add_transceiver(
                            audio.clone().into(),
                            RtpTransceiverInit {
                                direction: RtpTransceiverDirection::SendOnly,
                                stream_ids: vec!["zabor-screen".into()],
                                send_encodings: vec![RtpEncodingParameters {
                                    max_bitrate: Some(64_000),
                                    priority: Priority::High,
                                    ..Default::default()
                                }],
                            },
                        )
                        .map_err(|_| MediaError::Backend)?
                        .sender(),
                );
            }
        } else {
            for kind in [MediaType::Video, MediaType::Audio] {
                result
                    .connection
                    .add_transceiver_for_media(
                        kind,
                        RtpTransceiverInit {
                            direction: RtpTransceiverDirection::RecvOnly,
                            stream_ids: Vec::new(),
                            send_encodings: Vec::new(),
                        },
                    )
                    .map_err(|_| MediaError::Backend)?;
            }
            let live = result.lifetime.clone();
            let events = engine.events.clone();
            let id = peer.to_owned();
            result.connection.on_track(Some(Box::new(move |event| {
                if live.is_alive() {
                    let _ = events.try_send(MediaEvent::SfuTrack {
                        peer: id.clone(),
                        epoch,
                        track: event.track,
                        lifetime: live.clone(),
                    });
                }
            })));
        }
        Ok(result)
    }

    async fn offer(&self) -> Result<String, MediaError> {
        let offer = self
            .connection
            .create_offer(OfferOptions {
                ice_restart: false,
                offer_to_receive_audio: None,
                offer_to_receive_video: None,
            })
            .await
            .map_err(|_| MediaError::Backend)?;
        self.connection
            .set_local_description(offer.clone())
            .await
            .map_err(|_| MediaError::Backend)?;
        let deadline = Instant::now() + Duration::from_millis(1200);
        while self.lifetime.is_alive()
            && self.connection.ice_gathering_state() != IceGatheringState::Complete
            && Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if !self.lifetime.is_alive() {
            return Err(MediaError::Backend);
        }
        let candidates = self
            .candidates
            .lock()
            .map_err(|_| MediaError::Backend)?
            .clone();
        Ok(gathered_offer(
            &offer.to_string(),
            &candidates,
            self.connection.ice_gathering_state() == IceGatheringState::Complete,
        ))
    }

    async fn answer(&self, response: Value) -> Result<(), MediaError> {
        let ok = response
            .get("ok")
            .or_else(|| response.get("Ok"))
            .and_then(Value::as_bool)
            == Some(true);
        let sdp = response
            .get("answerSdp")
            .or_else(|| response.get("sdp"))
            .or_else(|| response.get("Sdp"))
            .and_then(Value::as_str)
            .filter(|sdp| !sdp.is_empty() && sdp.len() <= 256 * 1024);
        if !ok || !self.lifetime.is_alive() {
            return Err(MediaError::InvalidSignal);
        }
        let answer =
            SessionDescription::parse(sdp.ok_or(MediaError::InvalidSignal)?, SdpType::Answer)
                .map_err(|_| MediaError::InvalidSignal)?;
        self.connection
            .set_remote_description(answer)
            .await
            .map_err(|_| MediaError::Backend)
    }
}

fn gathered_offer(sdp: &str, candidates: &[(String, i32, String)], complete: bool) -> String {
    let mut sections = Vec::<Vec<&str>>::new();
    let mut session = Vec::new();
    for line in sdp.lines().map(str::trim_end) {
        if line.starts_with("m=") {
            sections.push(Vec::new());
        }
        if let Some(section) = sections.last_mut() {
            section.push(line);
        } else {
            session.push(line);
        }
    }
    let mut output = session.join("\r\n") + "\r\n";
    for (index, section) in sections.iter().enumerate() {
        output.push_str(&section.join("\r\n"));
        output.push_str("\r\n");
        let mid = section.iter().find_map(|line| line.strip_prefix("a=mid:"));
        let mut added = BTreeSet::new();
        for (candidate_mid, candidate_index, candidate) in candidates {
            if (mid == Some(candidate_mid.as_str())
                || (candidate_mid.is_empty() && *candidate_index == index as i32))
                && candidate.starts_with("candidate:")
                && !candidate.contains(['\r', '\n'])
                && added.insert(candidate)
            {
                output.push_str("a=");
                output.push_str(candidate);
                output.push_str("\r\n");
            }
        }
        if complete && !section.contains(&"a=end-of-candidates") {
            output.push_str("a=end-of-candidates\r\n");
        }
    }
    output
}

impl MediaEngine {
    pub(crate) fn video_quality(&self) -> Option<&'static str> {
        self.video_profile.map(|profile| match profile {
            VideoSendProfile::Low => "low",
            VideoSendProfile::Medium => "medium",
            VideoSendProfile::High => "high",
            VideoSendProfile::Camera => "camera",
        })
    }

    pub(crate) fn event_sender(&self) -> mpsc::Sender<MediaEvent> {
        self.events.clone()
    }

    pub(crate) fn reset_sfu_publisher(&mut self) {
        self.sfu.publisher = None;
        self.sfu.epoch = 0;
        self.sfu.active = false;
        self.sfu.ready.clear();
        self.sfu.fallback.clear();
        self.sfu.close_at = None;
    }

    pub(crate) fn retain_sfu_viewers(&mut self) {
        self.sfu.ready.retain(|peer| self.allowed.contains(peer));
        self.sfu.fallback.retain(|peer| self.allowed.contains(peer));
    }

    pub fn set_viewer_state(&mut self, peer: &str, state: ViewerState) -> Result<(), MediaError> {
        if !self.allowed.contains(peer) {
            return Err(MediaError::UnauthorizedPeer);
        }
        if self.viewers.insert(peer.into(), state) == Some(state) || self.sfu.active {
            return Ok(());
        }
        let Some(quality) = self.video_quality() else {
            return Ok(());
        };
        if let Some(peer) = self.peers.get_mut(peer) {
            apply_profile(
                peer.video_sender.as_ref(),
                &mut peer.adaptive,
                quality,
                state,
            )?;
        }
        Ok(())
    }

    pub fn set_screen_sender_active(&mut self, id: &str, active: bool) -> Result<(), MediaError> {
        let quality = self.video_quality();
        let viewer = self.viewers.get(id).copied().unwrap_or_default();
        let peer = self.peers.get_mut(id).ok_or(MediaError::UnauthorizedPeer)?;
        for sender in [&peer.video_sender, &peer.stream_sender]
            .into_iter()
            .flatten()
        {
            if sender.track().is_none() {
                continue;
            }
            let mut parameters = sender.parameters();
            if let Some(encoding) = parameters.encodings.first_mut() {
                encoding.active = active;
            }
            sender
                .set_parameters(parameters)
                .map_err(|_| MediaError::Backend)?;
        }
        if active {
            peer.adaptive.applied = None;
            if let Some(quality) = quality {
                apply_profile(
                    peer.video_sender.as_ref(),
                    &mut peer.adaptive,
                    quality,
                    viewer,
                )?;
            }
        }
        Ok(())
    }

    pub async fn poll_video_stats(&mut self) {
        let Some(quality) = self.video_quality().filter(|quality| *quality != "camera") else {
            return;
        };
        for (id, peer) in &mut self.peers {
            if peer.video_sender.is_none()
                || peer.connection.connection_state() != PeerConnectionState::Connected
            {
                continue;
            }
            if let Ok(Ok(stats)) =
                tokio::time::timeout(Duration::from_secs(1), peer.connection.get_stats()).await
            {
                if peer.lifetime.is_alive() {
                    peer.adaptive.stats(&stats);
                    let _ = apply_profile(
                        peer.video_sender.as_ref(),
                        &mut peer.adaptive,
                        quality,
                        self.viewers.get(id).copied().unwrap_or_default(),
                    );
                }
            }
        }
        if let Some(peer) = self.sfu.publisher.as_mut().filter(|peer| {
            self.sfu.active && peer.connection.connection_state() == PeerConnectionState::Connected
        }) {
            if let Ok(Ok(stats)) =
                tokio::time::timeout(Duration::from_secs(1), peer.connection.get_stats()).await
            {
                if peer.lifetime.is_alive() {
                    peer.adaptive.stats(&stats);
                    let _ = apply_profile(
                        peer.video_sender.as_ref(),
                        &mut peer.adaptive,
                        quality,
                        ViewerState::Watching,
                    );
                }
            }
        }
    }

    pub(crate) fn begin_sfu_publisher(&mut self, epoch: u64) -> Result<bool, MediaError> {
        if self.data_only
            || epoch == 0
            || epoch > 9_007_199_254_740_991
            || epoch < self.sfu.epoch
            || !self.has_live_video()
        {
            return Err(MediaError::InvalidSignal);
        }
        if self.sfu.publisher.as_ref().is_some_and(|peer| {
            peer.epoch == epoch
                && !matches!(
                    peer.connection.connection_state(),
                    PeerConnectionState::Closed | PeerConnectionState::Failed
                )
        }) {
            return Ok(false);
        }
        let peer = SfuPeer::new(self, &self.local_id, epoch, true)?;
        self.sfu.publisher = Some(peer);
        self.sfu.epoch = epoch;
        self.sfu.close_at = None;
        self.sfu.ready.clear();
        self.sfu.fallback.clear();
        for id in self.peers.keys().cloned().collect::<Vec<_>>() {
            let _ = self.set_screen_sender_active(&id, true);
        }
        Ok(true)
    }

    pub(crate) fn begin_sfu_subscriber(
        &mut self,
        peer: &str,
        epoch: u64,
    ) -> Result<bool, MediaError> {
        if self.data_only || !self.allowed.contains(peer) {
            return Err(MediaError::UnauthorizedPeer);
        }
        if epoch == 0 || epoch > 9_007_199_254_740_991 {
            return Err(MediaError::InvalidSignal);
        }
        if let Some(existing) = self.sfu.subscribers.get(peer) {
            if existing.epoch > epoch {
                return Err(MediaError::InvalidSignal);
            }
            if existing.epoch == epoch
                && !matches!(
                    existing.connection.connection_state(),
                    PeerConnectionState::Failed | PeerConnectionState::Closed
                )
            {
                return Ok(false);
            }
        }
        let connection = SfuPeer::new(self, peer, epoch, false)?;
        self.sfu.subscribers.insert(peer.into(), connection);
        Ok(true)
    }

    pub(crate) fn sfu_lifetime(&self, peer: &str, publisher: bool) -> Option<PeerLifetime> {
        self.sfu_peer(peer, publisher)
            .map(|peer| peer.lifetime.clone())
    }

    fn sfu_peer(&self, peer: &str, publisher: bool) -> Option<&SfuPeer> {
        if publisher {
            self.sfu
                .publisher
                .as_ref()
                .filter(|_| peer == self.local_id)
        } else {
            self.sfu.subscribers.get(peer)
        }
    }

    pub(crate) async fn sfu_offer(
        &self,
        peer: &str,
        publisher: bool,
    ) -> Result<String, MediaError> {
        self.sfu_peer(peer, publisher)
            .ok_or(MediaError::Backend)?
            .offer()
            .await
    }

    pub(crate) async fn sfu_answer(
        &self,
        peer: &str,
        publisher: bool,
        response: Value,
    ) -> Result<(), MediaError> {
        self.sfu_peer(peer, publisher)
            .ok_or(MediaError::Backend)?
            .answer(response)
            .await
    }

    pub(crate) fn sfu_has_audio(&self) -> bool {
        self.sfu
            .publisher
            .as_ref()
            .is_some_and(|peer| peer.audio_sender.is_some())
    }

    pub(crate) fn sfu_epoch_matches(
        &self,
        peer: &str,
        publisher: bool,
        epoch: u64,
        lifetime: &PeerLifetime,
    ) -> bool {
        self.sfu_peer(peer, publisher).is_some_and(|peer| {
            peer.epoch == epoch && peer.lifetime.is_same(lifetime) && lifetime.is_alive()
        })
    }

    pub(crate) fn sfu_connected(&mut self, peer: &str, publisher: bool) -> bool {
        let peer = if publisher {
            self.sfu.publisher.as_mut()
        } else {
            self.sfu.subscribers.get_mut(peer)
        };
        if let Some(peer) = peer {
            if !peer.ready && publisher {
                peer.ready = true;
                return true;
            }
        }
        false
    }

    pub(crate) fn sfu_video_ready(&mut self, peer: &str, watching: bool) {
        if let Some(peer) = self.sfu.subscribers.get_mut(peer) {
            peer.ready = true;
            peer.preview_close = (!watching).then(|| Instant::now() + Duration::from_millis(2600));
        }
    }

    pub(crate) fn sfu_preview(&mut self, peer: &str, watching: bool) {
        if let Some(peer) = self.sfu.subscribers.get_mut(peer) {
            peer.preview_close = if watching || !peer.ready {
                None
            } else {
                Some(Instant::now() + Duration::from_millis(2600))
            };
        }
    }

    pub(crate) fn close_sfu_subscriber(&mut self, peer: &str) {
        self.sfu.subscribers.remove(peer);
    }

    pub(crate) fn set_delivery_mode(
        &mut self,
        mode: DeliveryMode,
        epoch: u64,
    ) -> Result<(), MediaError> {
        if epoch < self.sfu.epoch {
            return Ok(());
        }
        self.sfu.active = mode == DeliveryMode::Sfu;
        self.sfu.epoch = epoch;
        if self.sfu.active {
            self.sfu.close_at = None;
        } else {
            self.sfu.ready.clear();
            self.sfu.fallback.clear();
            self.sfu.close_at = Some(Instant::now() + Duration::from_millis(650));
            for peer in self.peers.keys().cloned().collect::<Vec<_>>() {
                let _ = self.set_screen_sender_active(&peer, true);
            }
        }
        Ok(())
    }

    pub(crate) fn viewer_sfu_ready(
        &mut self,
        peer: &str,
        epoch: u64,
        fallback: bool,
    ) -> Result<(), MediaError> {
        if !self.allowed.contains(peer) {
            return Err(MediaError::UnauthorizedPeer);
        }
        if epoch != self.sfu.epoch
            || (!fallback && !self.sfu.active)
            || !self.has_live_video()
            || !self
                .sfu
                .publisher
                .as_ref()
                .is_some_and(|publisher| publisher.epoch == epoch)
        {
            return Ok(());
        }
        if fallback {
            self.sfu.ready.remove(peer);
            self.sfu.fallback.insert(peer.into());
        } else {
            self.sfu.ready.insert(peer.into());
        }
        if self.peers.contains_key(peer) {
            self.set_screen_sender_active(peer, fallback || self.sfu.fallback.contains(peer))?;
        }
        Ok(())
    }

    pub(crate) fn sfu_expired(&mut self, now: Instant) -> (Vec<(String, u64, bool)>, Vec<String>) {
        if self.sfu.close_at.is_some_and(|deadline| now >= deadline) && !self.sfu.active {
            self.sfu.publisher = None;
            self.sfu.close_at = None;
        }
        let mut failed = Vec::new();
        let mut previews = Vec::new();
        if let Some(peer) = self.sfu.publisher.as_mut() {
            if !peer.ready && !peer.failure_reported && now >= peer.deadline {
                peer.failure_reported = true;
                failed.push((self.local_id.clone(), peer.epoch, true));
            }
        }
        for (id, peer) in &mut self.sfu.subscribers {
            if !peer.ready && !peer.failure_reported && now >= peer.deadline {
                peer.failure_reported = true;
                failed.push((id.clone(), peer.epoch, false));
            }
            if peer.preview_close.is_some_and(|deadline| now >= deadline) {
                previews.push(id.clone());
                peer.preview_close = None;
            }
        }
        (failed, previews)
    }

    pub(crate) fn remove_sfu_audio(&mut self) {
        if let Some(peer) = self.sfu.publisher.as_mut() {
            if let Some(sender) = peer.audio_sender.take() {
                let _ = peer.connection.remove_track(sender);
            }
        }
    }
}

fn apply_profile(
    sender: Option<&RtpSender>,
    adaptive: &mut AdaptiveVideo,
    quality: &str,
    viewer: ViewerState,
) -> Result<(), MediaError> {
    let (Some(sender), Some(profile)) = (
        sender,
        VideoProfile::for_quality(quality, adaptive.step, viewer, false)?,
    ) else {
        return Ok(());
    };
    if adaptive.applied == Some(profile) {
        return Ok(());
    }
    let mut parameters = sender.parameters();
    profile.apply(&mut parameters);
    sender
        .set_parameters(parameters)
        .map_err(|_| MediaError::Backend)?;
    adaptive.applied = Some(profile);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_dto_preserves_case_compatibility_and_rejects_unsafe_epochs() {
        for value in [
            json!({"streamerId":"peer","mode":"sfu","epoch":42}),
            json!({"StreamerId":"peer","Mode":"sfu","Epoch":42}),
        ] {
            assert_eq!(
                StreamDelivery::parse(&value).unwrap(),
                StreamDelivery {
                    streamer: "peer".into(),
                    mode: DeliveryMode::Sfu,
                    epoch: 42
                }
            );
        }
        for value in [
            json!({"streamerId":"","epoch":1}),
            json!({"streamerId":"peer","epoch":-1}),
            json!({"streamerId":"peer","epoch":9007199254740992u64}),
        ] {
            assert!(StreamDelivery::parse(&value).is_err());
        }
    }

    #[test]
    fn nontrickle_offer_places_candidates_in_the_exact_mid_without_line_injection() {
        let sdp = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=mid:v\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:a\r\n";
        let candidates = vec![
            ("a".into(), 0, "candidate:audio".into()),
            ("v".into(), 1, "candidate:video".into()),
            ("v".into(), 0, "candidate:video".into()),
            ("missing".into(), 0, "candidate:absent".into()),
            ("v".into(), 0, "candidate:bad\r\na=evil".into()),
        ];
        let result = gathered_offer(sdp, &candidates, true);
        assert!(result.contains("a=mid:v\r\na=candidate:video\r\na=end-of-candidates\r\nm=audio"));
        assert!(result.contains("a=mid:a\r\na=candidate:audio\r\na=end-of-candidates"));
        assert!(!result.contains("absent") && !result.contains("evil"));
    }

    fn source() -> NativeVideoSource {
        NativeVideoSource::new(
            libwebrtc::video_source::VideoResolution {
                width: 160,
                height: 96,
            },
            true,
        )
    }

    fn engine(audio: bool) -> (MediaEngine, mpsc::Receiver<MediaEvent>) {
        let (mut engine, events) = MediaEngine::new("owner".into());
        engine
            .attach_audio_source(crate::microphone::audio_source())
            .unwrap();
        engine.set_video_with_quality(source(), "medium").unwrap();
        if audio {
            engine
                .set_stream_audio(Some(NativeAudioSource::new(
                    libwebrtc::audio_source::AudioSourceOptions {
                        echo_cancellation: false,
                        noise_suppression: false,
                        auto_gain_control: false,
                    },
                    48000,
                    2,
                    0,
                )))
                .unwrap();
        }
        engine.authorize_peers(["peer".into()]);
        engine
            .create_data_channel("peer", "sfu-ownership-fixture", DataChannelInit::default())
            .unwrap();
        (engine, events)
    }

    #[tokio::test]
    async fn publisher_offer_contains_only_share_video_and_optional_audio_never_microphone() {
        for audio in [false, true] {
            let (mut engine, _) = engine(audio);
            let microphone = engine.microphone.clone().unwrap();
            let p2p = engine.peer_lifetime("peer").unwrap();
            engine.begin_sfu_publisher(42).unwrap();
            let offer = engine.sfu_offer("owner", true).await.unwrap();
            assert_eq!(
                offer
                    .lines()
                    .filter(|line| line.starts_with("m=video "))
                    .count(),
                1
            );
            assert_eq!(
                offer
                    .lines()
                    .filter(|line| line.starts_with("m=audio "))
                    .count(),
                usize::from(audio)
            );
            assert!(!offer.contains("zabor-microphone") && !offer.contains("zabor-voice"));
            assert_eq!(engine.sfu_has_audio(), audio);
            let publisher = engine.sfu.publisher.as_ref().unwrap();
            assert!(publisher
                .connection
                .transceivers()
                .iter()
                .all(|transceiver| transceiver.direction() == RtpTransceiverDirection::SendOnly));
            assert_eq!(
                publisher
                    .video_sender
                    .as_ref()
                    .unwrap()
                    .parameters()
                    .encodings[0]
                    .max_bitrate,
                Some(3_000_000)
            );
            assert!(microphone.enabled() && p2p.is_alive());
            assert!(engine.peer_lifetime("peer").unwrap().is_same(&p2p));
        }
    }

    #[tokio::test]
    async fn subscriber_offer_is_receive_only_even_when_the_client_has_its_own_microphone_and_share(
    ) {
        let (mut engine, _) = engine(true);
        engine.begin_sfu_subscriber("peer", 42).unwrap();
        let subscriber = &engine.sfu.subscribers["peer"];
        assert_eq!(subscriber.connection.transceivers().len(), 2);
        assert!(subscriber
            .connection
            .transceivers()
            .iter()
            .all(
                |transceiver| transceiver.direction() == RtpTransceiverDirection::RecvOnly
                    && transceiver.sender().track().is_none()
            ));
        let offer = engine.sfu_offer("peer", false).await.unwrap();
        assert!(!offer.contains("zabor-microphone") && !offer.contains("zabor-screen"));
        assert!(engine.microphone.as_ref().unwrap().enabled());
    }

    #[tokio::test]
    async fn sfu_rejects_unknown_peers_data_only_engines_and_stale_or_unsafe_epochs() {
        let (mut engine, _) = engine(false);
        assert_eq!(
            engine.begin_sfu_subscriber("stranger", 1),
            Err(MediaError::UnauthorizedPeer)
        );
        for epoch in [0, 9_007_199_254_740_992] {
            assert!(engine.begin_sfu_publisher(epoch).is_err());
        }
        engine.begin_sfu_publisher(20).unwrap();
        let original = engine.sfu_lifetime("owner", true).unwrap();
        assert!(!engine.begin_sfu_publisher(20).unwrap());
        assert!(engine.begin_sfu_publisher(19).is_err());
        assert!(original.is_alive());
        engine.begin_sfu_publisher(21).unwrap();
        assert!(!original.is_alive());
        assert!(!engine.sfu_epoch_matches("owner", true, 20, &original));
        let (mut data, _) = MediaEngine::new_data_only("owner".into());
        data.authorize_peers(["peer".into()]);
        assert!(data.begin_sfu_subscriber("peer", 1).is_err());
    }

    #[tokio::test]
    async fn sfu_viewer_ready_and_fallback_change_only_share_senders_and_preserve_voice() {
        let (mut engine, _) = engine(true);
        let microphone = engine.microphone.clone().unwrap();
        let p2p = engine.peer_lifetime("peer").unwrap();
        engine.begin_sfu_publisher(42).unwrap();
        engine.set_delivery_mode(DeliveryMode::Sfu, 42).unwrap();
        engine.viewer_sfu_ready("peer", 41, false).unwrap();
        assert!(
            engine.peers["peer"]
                .video_sender
                .as_ref()
                .unwrap()
                .parameters()
                .encodings[0]
                .active
        );
        engine.viewer_sfu_ready("peer", 42, false).unwrap();
        let peer = &engine.peers["peer"];
        assert!(!peer.video_sender.as_ref().unwrap().parameters().encodings[0].active);
        assert!(!peer.stream_sender.as_ref().unwrap().parameters().encodings[0].active);
        assert!(
            peer.connection
                .senders()
                .iter()
                .find(|sender| sender
                    .track()
                    .is_some_and(|track| track.id() == microphone.id()))
                .unwrap()
                .parameters()
                .encodings[0]
                .active
        );
        engine.viewer_sfu_ready("peer", 42, true).unwrap();
        assert!(
            engine.peers["peer"]
                .video_sender
                .as_ref()
                .unwrap()
                .parameters()
                .encodings[0]
                .active
        );
        assert!(microphone.enabled() && p2p.is_alive());
        engine.viewer_sfu_ready("peer", 42, false).unwrap();
        assert!(
            engine.peers["peer"]
                .video_sender
                .as_ref()
                .unwrap()
                .parameters()
                .encodings[0]
                .active
        );
    }

    #[tokio::test]
    async fn replacing_a_local_share_keeps_the_independent_remote_subscriber_and_camera_defaults() {
        let (mut engine, _) = engine(false);
        engine.begin_sfu_publisher(42).unwrap();
        engine.begin_sfu_subscriber("peer", 51).unwrap();
        let subscriber = engine.sfu_lifetime("peer", false).unwrap();
        let publisher = engine.sfu_lifetime("owner", true).unwrap();
        engine
            .set_viewer_state("peer", ViewerState::Preview)
            .unwrap();
        engine.set_video_with_quality(source(), "camera").unwrap();
        assert!(!publisher.is_alive() && subscriber.is_alive());
        assert!(engine
            .sfu_lifetime("peer", false)
            .unwrap()
            .is_same(&subscriber));
        let params = engine.peers["peer"]
            .video_sender
            .as_ref()
            .unwrap()
            .parameters();
        assert!(params.encodings[0].scale_resolution_down_by.is_none());
        assert!(params.encodings[0].max_bitrate.is_none());
    }

    #[tokio::test]
    async fn preview_snapshot_close_watchdog_and_publisher_hold_have_separate_deadlines() {
        let (mut engine, _) = engine(false);
        let now = Instant::now();
        engine.begin_sfu_publisher(42).unwrap();
        engine.begin_sfu_subscriber("peer", 42).unwrap();
        assert!(engine
            .sfu_expired(now + Duration::from_secs(6))
            .0
            .is_empty());
        let failed = engine.sfu_expired(now + Duration::from_secs(7)).0;
        assert_eq!(failed, vec![("owner".into(), 42, true)]);
        assert_eq!(
            engine.sfu_expired(now + Duration::from_secs(11)).0,
            vec![("peer".into(), 42, false)]
        );
        assert!(engine
            .sfu_expired(now + Duration::from_secs(12))
            .0
            .is_empty());
        engine.sfu_video_ready("peer", false);
        assert!(engine
            .sfu_expired(Instant::now() + Duration::from_millis(2500))
            .1
            .is_empty());
        assert_eq!(
            engine
                .sfu_expired(Instant::now() + Duration::from_millis(2700))
                .1,
            vec!["peer".to_owned()]
        );
        engine.sfu_preview("peer", true);
        assert!(engine
            .sfu_expired(Instant::now() + Duration::from_secs(20))
            .1
            .is_empty());
        let lifetime = engine.sfu_lifetime("owner", true).unwrap();
        engine.set_delivery_mode(DeliveryMode::P2p, 42).unwrap();
        engine.sfu_expired(Instant::now() + Duration::from_millis(600));
        assert!(lifetime.is_alive());
        engine.sfu_expired(Instant::now() + Duration::from_millis(700));
        assert!(!lifetime.is_alive());
    }

    #[tokio::test]
    async fn native_sfu_receive_only_path_decodes_local_rtp_without_voice_or_physical_capture() {
        use futures_util::StreamExt;
        use libwebrtc::video_frame::{I420Buffer, VideoFrame, VideoRotation};
        use libwebrtc::video_stream::native::NativeVideoStream;
        tokio::time::timeout(Duration::from_secs(15), async {
            let (mut publisher, _) = engine(false);
            let source = source();
            publisher.set_video_with_quality(source.clone(), "low").unwrap();
            publisher.begin_sfu_publisher(42).unwrap();
            let offer = publisher.sfu_offer("owner", true).await.unwrap();
            let (mut receiver, mut events) = MediaEngine::new("viewer".into());
            receiver.authorize_peers(["owner".into()]);
            receiver.begin_sfu_subscriber("owner", 42).unwrap();
            let subscriber = &receiver.sfu.subscribers["owner"];
            subscriber.connection.set_remote_description(SessionDescription::parse(&offer, SdpType::Offer).unwrap()).await.unwrap();
            let answer = subscriber.connection.create_answer(AnswerOptions::default()).await.unwrap();
            subscriber.connection.set_local_description(answer.clone()).await.unwrap();
            let deadline = Instant::now() + Duration::from_millis(1200);
            while subscriber.connection.ice_gathering_state() != IceGatheringState::Complete && Instant::now() < deadline { tokio::time::sleep(Duration::from_millis(20)).await; }
            let answer = gathered_offer(&answer.to_string(), &subscriber.candidates.lock().unwrap(), true);
            publisher.sfu_answer("owner", true, json!({"ok":true,"answerSdp":answer})).await.unwrap();
            let track = loop {
                if let MediaEvent::SfuTrack { track: MediaStreamTrack::Video(track), epoch: 42, lifetime, .. } = events.recv().await.unwrap() {
                    assert!(lifetime.is_alive()); break track;
                }
            };
            let mut stream = NativeVideoStream::new(track);
            let mut ticker = tokio::time::interval(Duration::from_millis(33));
            loop {
                tokio::select! {
                    _ = ticker.tick() => { source.capture_frame(&VideoFrame::new(VideoRotation::VideoRotation0, I420Buffer::new_black(160, 96))); },
                    frame = stream.next() => {
                        let decoded = crate::video::decode_frame(frame.unwrap()).unwrap();
                        assert_eq!((decoded.width, decoded.height), (160, 96)); break;
                    }
                }
            }
            assert!(receiver.microphone.is_none());
            stream.close();
        }).await.unwrap();
    }
}
