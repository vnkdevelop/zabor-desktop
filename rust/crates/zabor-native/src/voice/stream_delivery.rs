use super::*;
use crate::media::adaptive::ViewerState;
use crate::media::{DeliveryMode, StreamDelivery};
use crate::video::VideoReceiver;
use libwebrtc::media_stream_track::MediaStreamTrack;
use libwebrtc::peer_connection::PeerConnectionState;
use libwebrtc::video_track::RtcVideoTrack;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

type VideoBackup = (RtcVideoTrack, PeerLifetime);
type AudioBackup = (RtcAudioTrack, PeerLifetime);

pub(super) struct StreamRoutes {
    delivery: BTreeMap<String, StreamDelivery>,
    pub(super) pending_publish: Option<u64>,
    streaming: BTreeSet<String>,
    focus: Option<String>,
    control: Option<String>,
    reported: BTreeMap<String, ViewerState>,
    p2p_video: BTreeMap<String, VideoBackup>,
    pub(super) p2p_audio: BTreeMap<String, AudioBackup>,
    sfu_audio: BTreeMap<String, AudioBackup>,
    pub(super) selected: BTreeMap<String, Option<u64>>,
    probes: BTreeMap<String, VideoReceiver>,
    probe_deadlines: BTreeMap<String, (Instant, Option<u64>)>,
    snapshots: BTreeSet<String>,
    last_stats: Instant,
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::voice::stream_tests::{fixture, peer, session, share};
    use libwebrtc::peer_connection_factory::{
        native::PeerConnectionFactoryExt, PeerConnectionFactory,
    };
    use libwebrtc::video_frame::{I420Buffer, VideoFrame, VideoRotation};
    use libwebrtc::video_source::{native::NativeVideoSource, VideoResolution};
    use tokio::time::timeout;

    #[tokio::test]
    async fn interest_reports_only_authorized_streams_and_only_the_focused_peer_is_watching() {
        let fixture = fixture([]).await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let mut session = session();
        session
            .handle(&hub, "UserStartedStreaming", &[json!("stranger")])
            .await
            .unwrap();
        session
            .handle(&hub, "UserStartedStreaming", &[json!("peer")])
            .await
            .unwrap();
        session.maintain_streams(&hub).await;
        session
            .set_stream_focus(&hub, Some("peer".into()))
            .await
            .unwrap();
        session
            .set_stream_focus(&hub, Some("peer".into()))
            .await
            .unwrap();
        session
            .set_stream_focus(&hub, Some("stranger".into()))
            .await
            .unwrap();
        hub.close();
        let requests = fixture.server.await.unwrap();
        let states = requests
            .iter()
            .filter(|request| request["target"] == "SendStreamViewState")
            .map(|request| request["arguments"].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            states,
            vec![
                json!(["peer", "preview"]),
                json!(["peer", "watching"]),
                json!(["peer", "preview"])
            ]
        );
    }

    #[tokio::test]
    async fn deferred_sfu_publish_preserves_newest_epoch_and_refusal_keeps_original_share_and_voice(
    ) {
        let fixture = fixture([]).await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let mut session = session();
        session
            .handle(
                &hub,
                "StreamSfuPublishRequired",
                &[json!(21), json!("local-node")],
            )
            .await
            .unwrap();
        session
            .handle(
                &hub,
                "StreamSfuPublishRequired",
                &[json!(20), json!("stale-node")],
            )
            .await
            .unwrap();
        session.maintain_streams(&hub).await;
        assert_eq!(session.streams.pending_publish, Some(21));
        share(&mut session, "medium");
        let p2p = peer(&mut session);
        let track = session.engine.local_video().unwrap();
        session.maintain_streams(&hub).await;
        assert!(track.enabled() && p2p.is_alive() && session.is_sharing());
        assert!(session.engine.peer_lifetime("peer").unwrap().is_same(&p2p));
        assert_eq!(session.capture.as_ref().unwrap().quality, "medium");
        hub.close();
        let requests = fixture.server.await.unwrap();
        let publish = requests
            .iter()
            .find(|request| request["target"] == "SfuPublish")
            .unwrap();
        assert_eq!(publish["arguments"][0], 21);
        assert_eq!(publish["arguments"][2], false);
        assert!(!publish["arguments"][1]
            .as_str()
            .unwrap()
            .contains("zabor-microphone"));
        assert!(requests
            .iter()
            .any(|request| request["target"] == "SfuReportFailure"
                && request["arguments"] == json!(["owner", 21])));
        assert!(!requests.iter().any(
            |request| request["target"] == "StopStream" || request["target"] == "LeaveChannel"
        ));
    }

    #[tokio::test]
    async fn delivery_epochs_and_membership_reject_old_or_unrelated_sfu_instructions() {
        let fixture = fixture([]).await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let mut session = session();
        session
            .handle(
                &hub,
                "StreamDeliveryModeChanged",
                &[json!({"streamerId":"stranger","mode":"sfu","epoch":21})],
            )
            .await
            .unwrap();
        assert!(!session.streams.delivery.contains_key("stranger"));
        session
            .handle(
                &hub,
                "StreamDeliveryModeChanged",
                &[json!({"streamerId":"peer","mode":"p2p","epoch":21})],
            )
            .await
            .unwrap();
        session
            .handle(
                &hub,
                "StreamDeliveryModeChanged",
                &[json!({"streamerId":"peer","mode":"sfu","epoch":20})],
            )
            .await
            .unwrap();
        assert_eq!(session.streams.delivery["peer"].epoch, 21);
        assert!(session.engine.sfu_lifetime("peer", false).is_none());
        hub.close();
        assert!(fixture.server.await.unwrap().is_empty());
    }

    fn frames() -> (NativeVideoSource, RtcVideoTrack) {
        let source = NativeVideoSource::new(
            VideoResolution {
                width: 160,
                height: 96,
            },
            true,
        );
        let track = PeerConnectionFactory::default()
            .create_video_track("first-frame-fixture", source.clone());
        (source, track)
    }

    async fn ready(
        events: &mut mpsc::Receiver<MediaEvent>,
        source: &NativeVideoSource,
    ) -> MediaEvent {
        timeout(Duration::from_secs(5), async {
            let mut ticker = tokio::time::interval(Duration::from_millis(33));
            loop {
                tokio::select! {
                    _ = ticker.tick() => { source.capture_frame(&VideoFrame::new(VideoRotation::VideoRotation0, I420Buffer::new_black(160, 96))); },
                    event = events.recv() => { let event = event.unwrap(); if matches!(event, MediaEvent::StreamVideoReady { .. }) { return event; } }
                }
            }
        }).await.unwrap()
    }

    #[tokio::test]
    async fn first_frame_probe_uses_original_lease_and_cannot_accept_a_replaced_same_peer_callback()
    {
        let fixture = fixture([]).await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let mut session = session();
        let (engine, mut events) = MediaEngine::new("owner".into());
        session.engine = engine;
        let lifetime = peer(&mut session);
        let (source, track) = frames();
        session.prepare_p2p_video("peer", track.clone(), lifetime.clone());
        assert!(session.streams.selected.is_empty());
        let MediaEvent::StreamVideoReady { lease: old, .. } = ready(&mut events, &source).await
        else {
            panic!()
        };
        session.prepare_p2p_video("peer", track, lifetime.clone());
        assert!(!old.is_alive());
        assert!(
            !session
                .accept_stream_video_ready(&hub, "peer", None, &lifetime, &old)
                .await
        );
        let MediaEvent::StreamVideoReady { lease, .. } = ready(&mut events, &source).await else {
            panic!()
        };
        assert!(
            session
                .accept_stream_video_ready(&hub, "peer", None, &lifetime, &lease)
                .await
        );
        assert!(!lease.is_alive());
        assert_eq!(session.streams.selected["peer"], None);
        let shared = session.audio_runtime.clone();
        drop(session);
        shared.wait_for_cleanup().await.unwrap();
        hub.close();
        fixture.server.await.unwrap();
    }

    #[tokio::test]
    async fn sfu_viewer_ready_is_sent_only_after_decoded_frame_and_never_for_a_preview_snapshot() {
        for watching in [false, true] {
            let fixture = fixture([]).await;
            let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
                .await
                .unwrap();
            let mut session = session();
            let (engine, mut events) = MediaEngine::new("owner".into());
            session.engine = engine;
            peer(&mut session);
            session.engine.begin_sfu_subscriber("peer", 42).unwrap();
            let lifetime = session.engine.sfu_lifetime("peer", false).unwrap();
            session.streams.delivery.insert(
                "peer".into(),
                StreamDelivery {
                    streamer: "peer".into(),
                    mode: DeliveryMode::Sfu,
                    epoch: 42,
                },
            );
            session.streams.focus = watching.then(|| "peer".into());
            let (source, track) = frames();
            session
                .handle_sfu_event(
                    &hub,
                    MediaEvent::SfuTrack {
                        peer: "peer".into(),
                        epoch: 42,
                        track: track.into(),
                        lifetime: lifetime.clone(),
                    },
                )
                .await
                .unwrap();
            assert!(session.streams.selected.is_empty());
            let MediaEvent::StreamVideoReady { lease, .. } = ready(&mut events, &source).await
            else {
                panic!()
            };
            assert!(
                session
                    .accept_stream_video_ready(&hub, "peer", Some(42), &lifetime, &lease)
                    .await
            );
            let shared = session.audio_runtime.clone();
            drop(session);
            shared.wait_for_cleanup().await.unwrap();
            assert!(!lifetime.is_alive());
            hub.close();
            let requests = fixture.server.await.unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request["target"] == "SfuViewerReady")
                    .count(),
                usize::from(watching)
            );
        }
    }

    #[tokio::test]
    async fn late_p2p_track_keeps_pending_sfu_probe_and_remains_available_for_fresh_fallback() {
        let fixture = fixture([]).await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let mut session = session();
        let (engine, mut events) = MediaEngine::new("owner".into());
        session.engine = engine;
        let p2p = peer(&mut session);
        session.engine.begin_sfu_subscriber("peer", 42).unwrap();
        let sfu = session.engine.sfu_lifetime("peer", false).unwrap();
        session.streams.delivery.insert(
            "peer".into(),
            StreamDelivery {
                streamer: "peer".into(),
                mode: DeliveryMode::Sfu,
                epoch: 42,
            },
        );
        let (sfu_source, sfu_track) = frames();
        session
            .handle_sfu_event(
                &hub,
                MediaEvent::SfuTrack {
                    peer: "peer".into(),
                    epoch: 42,
                    track: sfu_track.into(),
                    lifetime: sfu.clone(),
                },
            )
            .await
            .unwrap();
        let (p2p_source, p2p_track) = frames();
        session.prepare_p2p_video("peer", p2p_track, p2p.clone());
        assert!(session.streams.probes["peer"].lifetime().is_same(&sfu));
        assert!(session.streams.p2p_video["peer"].1.is_same(&p2p));
        let MediaEvent::StreamVideoReady { lease, epoch, .. } =
            ready(&mut events, &sfu_source).await
        else {
            panic!()
        };
        assert_eq!(epoch, Some(42));
        assert!(
            session
                .accept_stream_video_ready(&hub, "peer", epoch, &sfu, &lease)
                .await
        );
        session.fallback_video("peer");
        assert!(!sfu.is_alive() && p2p.is_alive());
        assert!(session.has_stream_snapshot("peer"));
        let MediaEvent::StreamVideoReady { lease, epoch, .. } =
            ready(&mut events, &p2p_source).await
        else {
            panic!()
        };
        assert_eq!(epoch, None);
        assert!(
            session
                .accept_stream_video_ready(&hub, "peer", epoch, &p2p, &lease)
                .await
        );
        assert!(!session.has_stream_snapshot("peer"));
        assert_eq!(session.streams.selected["peer"], None);
        let shared = session.audio_runtime.clone();
        drop(session);
        shared.wait_for_cleanup().await.unwrap();
        hub.close();
        fixture.server.await.unwrap();
    }

    #[tokio::test]
    async fn remote_stream_stop_retires_only_stream_audio_and_backups_not_original_voice_peer() {
        use crate::audio_mix::AudioMixer;
        use crate::microphone::audio_source;
        let fixture = fixture([]).await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let mut session = session();
        let lifetime = peer(&mut session);
        let factory = PeerConnectionFactory::default();
        let voice = factory.create_audio_track("original-voice", audio_source());
        let audio = factory.create_audio_track("original-stream", audio_source());
        session.playback = Some(AudioPlayback::with_cleanup(
            Arc::new(AudioMixer::new(session.controls.clone())),
            session.audio_runtime.cleanup_tracker(),
        ));
        session
            .receive_audio("peer", voice, lifetime.clone(), false)
            .unwrap();
        session
            .receive_audio("peer", audio, lifetime.clone(), true)
            .unwrap();
        let (_, video) = frames();
        session.prepare_p2p_video("peer", video, lifetime.clone());
        assert_eq!(session.playback.as_ref().unwrap().subscription_count(), 2);
        session
            .handle(&hub, "UserStoppedStreaming", &[json!("peer")])
            .await
            .unwrap();
        assert_eq!(session.playback.as_ref().unwrap().subscription_count(), 1);
        assert!(session.streams.p2p_audio.is_empty() && session.streams.p2p_video.is_empty());
        assert!(session.streams.sfu_audio.is_empty() && session.streams.probes.is_empty());
        assert!(session.streams.probe_deadlines.is_empty() && session.streams.selected.is_empty());
        assert!(lifetime.is_alive() && session.has_peer("peer"));
        let runtime = session.audio_runtime.clone();
        drop(session);
        runtime.seal_cleanup().unwrap();
        runtime.wait_for_cleanup().await.unwrap();
        hub.close();
        assert!(fixture.server.await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn receive_audio_and_video_reject_unrelated_live_peer_and_unselected_sfu_originals() {
        use crate::audio_mix::AudioMixer;
        use crate::microphone::audio_source;
        let mut session = session();
        let original = peer(&mut session);
        let (mut unrelated, _) = MediaEngine::new_data_only("other".into());
        unrelated.authorize_peers(["peer".into()]);
        unrelated
            .create_data_channel(
                "peer",
                "unrelated",
                libwebrtc::data_channel::DataChannelInit::default(),
            )
            .unwrap();
        let wrong = unrelated.peer_lifetime("peer").unwrap();
        let factory = PeerConnectionFactory::default();
        let audio = factory.create_audio_track("synthetic-audio", audio_source());
        let (_, video) = frames();
        session.playback = Some(AudioPlayback::with_cleanup(
            Arc::new(AudioMixer::new(session.controls.clone())),
            session.audio_runtime.cleanup_tracker(),
        ));
        assert_eq!(
            session.receive_audio("peer", audio.clone(), wrong.clone(), false),
            Err(MediaError::UnauthorizedPeer)
        );
        assert_eq!(
            session.receive_audio("peer", audio.clone(), wrong.clone(), true),
            Err(MediaError::UnauthorizedPeer)
        );
        session.prepare_p2p_video("peer", video, wrong.clone());
        assert!(wrong.is_alive() && session.streams.p2p_video.is_empty());
        assert!(session.streams.p2p_audio.is_empty());
        session.engine.begin_sfu_subscriber("peer", 42).unwrap();
        let sfu = session.engine.sfu_lifetime("peer", false).unwrap();
        assert_eq!(
            session.receive_audio("peer", audio.clone(), sfu.clone(), true),
            Err(MediaError::UnauthorizedPeer)
        );
        session.streams.selected.insert("peer".into(), Some(42));
        session
            .receive_audio("peer", audio.clone(), sfu.clone(), true)
            .unwrap();
        let voice = factory.create_audio_track("synthetic-voice", audio_source());
        session
            .receive_audio("peer", voice, original.clone(), false)
            .unwrap();
        assert_eq!(session.playback.as_ref().unwrap().subscription_count(), 2);
        assert!(original.is_alive() && sfu.is_alive());
        let runtime = session.audio_runtime.clone();
        drop(session);
        runtime.seal_cleanup().unwrap();
        runtime.wait_for_cleanup().await.unwrap();
    }
}

impl Default for StreamRoutes {
    fn default() -> Self {
        Self {
            delivery: BTreeMap::new(),
            pending_publish: None,
            streaming: BTreeSet::new(),
            focus: None,
            control: None,
            reported: BTreeMap::new(),
            p2p_video: BTreeMap::new(),
            p2p_audio: BTreeMap::new(),
            sfu_audio: BTreeMap::new(),
            selected: BTreeMap::new(),
            probes: BTreeMap::new(),
            probe_deadlines: BTreeMap::new(),
            snapshots: BTreeSet::new(),
            last_stats: Instant::now(),
        }
    }
}

impl VoiceSession {
    pub(super) fn reset_stream_routes(&mut self) {
        #[cfg(windows)]
        self.reset_remote_control();
        self.streams = StreamRoutes::default();
        self.engine.reset_sfu_publisher();
    }

    pub(super) fn stream_members(&mut self, users: &[User]) {
        self.streams.streaming = users
            .iter()
            .filter(|user| user.is_streaming && self.peers.contains(&user.id))
            .map(|user| user.id.clone())
            .collect();
    }

    pub(super) fn stop_local_stream_route(&mut self) {
        self.streams.pending_publish = None;
        self.streams.delivery.remove(&self.local_id);
    }
    pub fn has_stream_snapshot(&self, peer: &str) -> bool {
        self.streams.snapshots.contains(peer)
    }

    fn watching(&self, peer: &str) -> bool {
        self.streams.focus.as_deref() == Some(peer)
    }

    pub(super) fn control_stream_focused(&self, peer: &str) -> bool {
        self.watching(peer) && self.streams.streaming.contains(peer)
    }

    pub(super) fn set_control_stream(&mut self, peer: Option<String>) {
        self.streams.control = peer;
    }

    pub(super) fn control_p2p_ready(&self, peer: &str) -> bool {
        self.streams.control.as_deref() == Some(peer)
            && self.streams.selected.get(peer) == Some(&None)
            && self
                .streams
                .p2p_video
                .get(peer)
                .is_some_and(|(_, lifetime)| lifetime.is_alive())
            && !self.streams.probes.contains_key(peer)
    }

    pub(super) async fn control_to_p2p(&mut self, hub: &HubConnection, peer: &str) {
        self.fallback_video(peer);
        let _ = hub.send("SfuUnsubscribe", vec![json!(peer)]).await;
        let _ = self.engine.send_remote_control(
            peer,
            zabor_input::Packet::Signal(zabor_input::Signal::KeyframeRequest),
        );
    }

    pub async fn set_stream_focus(
        &mut self,
        hub: &HubConnection,
        peer: Option<String>,
    ) -> Result<(), MediaError> {
        self.streams.focus = peer.filter(|peer| self.peers.contains(peer));
        self.focus_stream_audio(self.streams.focus.clone());
        let peers = self.streams.streaming.iter().cloned().collect::<Vec<_>>();
        for peer in peers {
            let watching = self.watching(&peer);
            self.engine.sfu_preview(&peer, watching);
            if watching && self.streams.control.as_deref() != Some(&peer) {
                if let Some(delivery) = self
                    .streams
                    .delivery
                    .get(&peer)
                    .filter(|delivery| delivery.mode == DeliveryMode::Sfu)
                    .cloned()
                {
                    self.subscribe_sfu(hub, &peer, delivery.epoch).await?;
                    let _ = hub.send("SfuRequestKeyframe", vec![json!(peer)]).await;
                }
            }
        }
        self.report_stream_interest(hub).await;
        Ok(())
    }

    async fn report_stream_interest(&mut self, hub: &HubConnection) {
        self.streams
            .streaming
            .retain(|peer| self.peers.contains(peer));
        self.streams
            .reported
            .retain(|peer, _| self.streams.streaming.contains(peer));
        for peer in self.streams.streaming.clone() {
            let state = if self.watching(&peer) {
                ViewerState::Watching
            } else {
                ViewerState::Preview
            };
            if self.streams.reported.get(&peer) != Some(&state)
                && hub
                    .send(
                        "SendStreamViewState",
                        vec![json!(peer), json!(state.as_str())],
                    )
                    .await
                    .is_ok()
            {
                self.streams.reported.insert(peer, state);
            }
        }
    }

    pub async fn maintain_streams(&mut self, hub: &HubConnection) {
        #[cfg(windows)]
        self.maintain_remote_control();
        for peer in self.engine.maintain_remote_channels() {
            let _ = self.renegotiate(hub, &peer).await;
        }
        self.retain_stream_peers();
        if self.is_sharing() {
            if let Some(epoch) = self.streams.pending_publish.take() {
                let _ = self.publish_sfu(hub, epoch).await;
            }
        }
        let expired = self
            .streams
            .probe_deadlines
            .iter()
            .filter(|(_, (deadline, _))| Instant::now() >= *deadline)
            .map(|(peer, (_, epoch))| (peer.clone(), *epoch))
            .collect::<Vec<_>>();
        for (peer, epoch) in expired {
            self.streams.probes.remove(&peer);
            self.streams.probe_deadlines.remove(&peer);
            if let Some(epoch) = epoch {
                self.sfu_failure(hub, &peer, epoch, false).await;
            }
        }
        let (failed, previews) = self.engine.sfu_expired(Instant::now());
        for (peer, epoch, publisher) in failed {
            self.sfu_failure(hub, &peer, epoch, publisher).await;
        }
        for peer in previews {
            if !self.watching(&peer) {
                self.streams.snapshots.insert(peer.clone());
                self.engine.close_sfu_subscriber(&peer);
                self.streams.probes.remove(&peer);
                self.streams.sfu_audio.remove(&peer);
                let _ = hub.send("SfuUnsubscribe", vec![json!(peer)]).await;
            }
        }
        self.report_stream_interest(hub).await;
        if self.streams.last_stats.elapsed() >= Duration::from_millis(2500) {
            self.streams.last_stats = Instant::now();
            self.engine.poll_video_stats().await;
        }
    }

    fn retain_stream_peers(&mut self) {
        self.streams
            .delivery
            .retain(|peer, _| peer == &self.local_id || self.peers.contains(peer));
        self.streams
            .streaming
            .retain(|peer| self.peers.contains(peer));
        self.streams
            .p2p_video
            .retain(|peer, (_, lifetime)| self.peers.contains(peer) && lifetime.is_alive());
        self.streams
            .p2p_audio
            .retain(|peer, (_, lifetime)| self.peers.contains(peer) && lifetime.is_alive());
        self.streams
            .sfu_audio
            .retain(|peer, (_, lifetime)| self.peers.contains(peer) && lifetime.is_alive());
        self.streams
            .probes
            .retain(|peer, probe| self.peers.contains(peer) && probe.is_alive());
        self.streams
            .snapshots
            .retain(|peer| self.peers.contains(peer));
        self.streams
            .selected
            .retain(|peer, _| self.peers.contains(peer));
        self.streams
            .probe_deadlines
            .retain(|peer, _| self.streams.probes.contains_key(peer));
    }

    async fn publish_sfu(&mut self, hub: &HubConnection, epoch: u64) -> Result<(), MediaError> {
        if !self.engine.begin_sfu_publisher(epoch)? {
            return Ok(());
        }
        let lifetime = self
            .engine
            .sfu_lifetime(&self.local_id, true)
            .ok_or(MediaError::Backend)?;
        let result = async {
            let offer = self.engine.sfu_offer(&self.local_id, true).await?;
            let response = hub
                .invoke(
                    "SfuPublish",
                    vec![
                        json!(epoch),
                        json!(offer),
                        json!(self.engine.sfu_has_audio()),
                    ],
                    Duration::from_secs(10),
                )
                .await
                .map_err(|_| MediaError::Backend)?;
            if !self
                .engine
                .sfu_epoch_matches(&self.local_id, true, epoch, &lifetime)
            {
                return Err(MediaError::Backend);
            }
            self.engine.sfu_answer(&self.local_id, true, response).await
        }
        .await;
        if result.is_err() {
            let _ = hub
                .send("SfuReportFailure", vec![json!(self.local_id), json!(epoch)])
                .await;
        }
        result
    }

    async fn subscribe_sfu(
        &mut self,
        hub: &HubConnection,
        peer: &str,
        epoch: u64,
    ) -> Result<(), MediaError> {
        if !self.engine.begin_sfu_subscriber(peer, epoch)? {
            return Ok(());
        }
        let lifetime = self
            .engine
            .sfu_lifetime(peer, false)
            .ok_or(MediaError::Backend)?;
        self.streams.probes.remove(peer);
        let result = async {
            let offer = self.engine.sfu_offer(peer, false).await?;
            let response = hub
                .invoke(
                    "SfuSubscribe",
                    vec![json!(peer), json!(epoch), json!(offer)],
                    Duration::from_secs(10),
                )
                .await
                .map_err(|_| MediaError::Backend)?;
            if !self.engine.sfu_epoch_matches(peer, false, epoch, &lifetime) {
                return Err(MediaError::Backend);
            }
            self.engine.sfu_answer(peer, false, response).await
        }
        .await;
        if result.is_err() {
            self.sfu_failure(hub, peer, epoch, false).await;
        }
        result
    }

    async fn sfu_failure(&mut self, hub: &HubConnection, peer: &str, epoch: u64, publisher: bool) {
        let _ = hub
            .send("SfuReportFailure", vec![json!(peer), json!(epoch)])
            .await;
        if publisher {
            let _ = self.engine.set_delivery_mode(DeliveryMode::P2p, epoch);
        } else {
            self.fallback_video(peer);
        }
    }

    fn fallback_video(&mut self, peer: &str) {
        self.streams.snapshots.insert(peer.into());
        self.engine.close_sfu_subscriber(peer);
        self.streams.probes.remove(peer);
        self.streams.selected.remove(peer);
        self.streams.sfu_audio.remove(peer);
        if let Some((track, lifetime)) = self
            .streams
            .p2p_video
            .get(peer)
            .cloned()
            .filter(|(_, lifetime)| lifetime.is_alive())
        {
            self.probe_video(peer, track, lifetime, None);
        }
    }

    fn probe_video(
        &mut self,
        peer: &str,
        track: RtcVideoTrack,
        lifetime: PeerLifetime,
        epoch: Option<u64>,
    ) {
        if !self.peers.contains(peer) || !lifetime.is_alive() {
            return;
        }
        let events = self.engine.event_sender();
        let delivered = AtomicBool::new(false);
        let ready_track = track.clone();
        let ready_lifetime = lifetime.clone();
        let id = peer.to_owned();
        #[cfg(windows)]
        let mut probe = VideoReceiver::with_audio_runtime(
            track,
            lifetime,
            Arc::new(move |delivery| {
                if delivery.lease.is_alive()
                    && !delivered.load(Ordering::Acquire)
                    && events
                        .try_send(MediaEvent::StreamVideoReady {
                            peer: id.clone(),
                            epoch,
                            track: ready_track.clone(),
                            lifetime: ready_lifetime.clone(),
                            lease: delivery.lease,
                        })
                        .is_ok()
                {
                    delivered.store(true, Ordering::Release);
                }
            }),
            self.audio_runtime.as_ref(),
        );
        #[cfg(not(windows))]
        let mut probe = VideoReceiver::new(
            track,
            lifetime,
            Arc::new(move |delivery| {
                if delivery.lease.is_alive()
                    && !delivered.load(Ordering::Acquire)
                    && events
                        .try_send(MediaEvent::StreamVideoReady {
                            peer: id.clone(),
                            epoch,
                            track: ready_track.clone(),
                            lifetime: ready_lifetime.clone(),
                            lease: delivery.lease,
                        })
                        .is_ok()
                {
                    delivered.store(true, Ordering::Release);
                }
            }),
        );
        probe.start();
        self.streams.probes.insert(peer.into(), probe);
        self.streams.probe_deadlines.insert(
            peer.into(),
            (Instant::now() + Duration::from_millis(6500), epoch),
        );
    }

    pub fn prepare_p2p_video(&mut self, peer: &str, track: RtcVideoTrack, lifetime: PeerLifetime) {
        if !self.peers.contains(peer)
            || !lifetime.is_alive()
            || !self
                .engine
                .peer_lifetime(peer)
                .is_some_and(|original| original.is_same(&lifetime))
        {
            return;
        }
        self.streams.streaming.insert(peer.into());
        self.streams
            .p2p_video
            .insert(peer.into(), (track.clone(), lifetime.clone()));
        let sfu_probe = self
            .streams
            .probe_deadlines
            .get(peer)
            .is_some_and(|(_, epoch)| epoch.is_some())
            && self
                .streams
                .probes
                .get(peer)
                .is_some_and(VideoReceiver::is_alive);
        if !self.streams.selected.get(peer).is_some_and(Option::is_some) && !sfu_probe {
            self.probe_video(peer, track, lifetime, None);
        }
    }

    pub async fn accept_stream_video_ready(
        &mut self,
        hub: &HubConnection,
        peer: &str,
        epoch: Option<u64>,
        lifetime: &PeerLifetime,
        lease: &crate::video::VideoFrameLease,
    ) -> bool {
        if !lease.is_alive()
            || !self.peers.contains(peer)
            || !lifetime.is_alive()
            || !self
                .streams
                .probes
                .get(peer)
                .is_some_and(|probe| probe.lifetime().is_same(lifetime))
        {
            return false;
        }
        if let Some(epoch) = epoch {
            if !self.engine.sfu_epoch_matches(peer, false, epoch, lifetime)
                || !self.streams.delivery.get(peer).is_some_and(|delivery| {
                    delivery.mode == DeliveryMode::Sfu && delivery.epoch == epoch
                })
            {
                return false;
            }
            self.engine.sfu_video_ready(peer, self.watching(peer));
            if self.watching(peer) {
                let _ = hub
                    .send("SfuViewerReady", vec![json!(peer), json!(epoch)])
                    .await;
            }
        }
        self.streams.probes.remove(peer);
        self.streams.probe_deadlines.remove(peer);
        self.streams.selected.insert(peer.into(), epoch);
        self.streams.snapshots.remove(peer);
        let audio = if epoch.is_some() {
            self.streams.sfu_audio.get(peer)
        } else {
            self.streams.p2p_audio.get(peer)
        }
        .cloned();
        if let Some((track, lifetime)) = audio {
            let _ = self.receive_audio(peer, track, lifetime, true);
        }
        true
    }

    pub async fn handle_sfu_event(
        &mut self,
        hub: &HubConnection,
        event: MediaEvent,
    ) -> Result<(), MediaError> {
        match event {
            MediaEvent::SfuConnection {
                peer,
                epoch,
                publisher,
                state,
                lifetime,
            } => {
                if !self
                    .engine
                    .sfu_epoch_matches(&peer, publisher, epoch, &lifetime)
                {
                    return Ok(());
                }
                if state == PeerConnectionState::Connected {
                    if publisher && self.engine.sfu_connected(&peer, true) {
                        let _ = hub.send("SfuPublishReady", vec![json!(epoch)]).await;
                    }
                    if !publisher {
                        let _ = hub.send("SfuRequestKeyframe", vec![json!(peer)]).await;
                    }
                } else if state == PeerConnectionState::Failed {
                    self.sfu_failure(hub, &peer, epoch, publisher).await;
                }
            }
            MediaEvent::SfuTrack {
                peer,
                epoch,
                track,
                lifetime,
            } => {
                if self.streams.control.as_deref() == Some(&peer) {
                    return Ok(());
                }
                if !self
                    .engine
                    .sfu_epoch_matches(&peer, false, epoch, &lifetime)
                    || !self.peers.contains(&peer)
                {
                    return Ok(());
                }
                self.streams.streaming.insert(peer.clone());
                match track {
                    MediaStreamTrack::Video(track) => {
                        self.probe_video(&peer, track, lifetime, Some(epoch))
                    }
                    MediaStreamTrack::Audio(track) => {
                        self.streams
                            .sfu_audio
                            .insert(peer.clone(), (track.clone(), lifetime.clone()));
                        if self.streams.selected.get(&peer) == Some(&Some(epoch)) {
                            self.receive_audio(&peer, track, lifetime, true)?;
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) async fn handle_stream_signal(
        &mut self,
        hub: &HubConnection,
        target: &str,
        args: &[Value],
    ) -> Result<(), MediaError> {
        match target {
            "ReceiveStreamViewState" => {
                if let (Some(peer), Some(state)) = (
                    args.first().and_then(Value::as_str),
                    args.get(1).and_then(Value::as_str),
                ) {
                    if self.peers.contains(peer) {
                        self.engine
                            .set_viewer_state(peer, ViewerState::parse(state))?;
                    }
                }
            }
            "StreamSfuPublishRequired" => {
                if let Some(epoch) = args
                    .first()
                    .and_then(Value::as_u64)
                    .filter(|epoch| *epoch > 0 && *epoch <= 9_007_199_254_740_991)
                {
                    if self.is_sharing() {
                        let _ = self.publish_sfu(hub, epoch).await;
                    } else if self
                        .streams
                        .pending_publish
                        .is_none_or(|previous| epoch >= previous)
                    {
                        self.streams.pending_publish = Some(epoch);
                    }
                }
            }
            "StreamDeliveryModeChanged" => {
                let delivery =
                    StreamDelivery::parse(args.first().ok_or(MediaError::InvalidSignal)?)?;
                if delivery.streamer != self.local_id && !self.peers.contains(&delivery.streamer) {
                    return Ok(());
                }
                if self
                    .streams
                    .delivery
                    .get(&delivery.streamer)
                    .is_some_and(|previous| previous.epoch > delivery.epoch)
                {
                    return Ok(());
                }
                self.streams
                    .delivery
                    .insert(delivery.streamer.clone(), delivery.clone());
                if self.streams.control.as_deref() == Some(&delivery.streamer) {
                    return Ok(());
                }
                if delivery.streamer == self.local_id {
                    self.engine
                        .set_delivery_mode(delivery.mode, delivery.epoch)?;
                } else if delivery.mode == DeliveryMode::Sfu {
                    self.streams.streaming.insert(delivery.streamer.clone());
                    if self.watching(&delivery.streamer)
                        || (!self.streams.selected.contains_key(&delivery.streamer)
                            && !self.streams.snapshots.contains(&delivery.streamer))
                    {
                        let _ = self
                            .subscribe_sfu(hub, &delivery.streamer, delivery.epoch)
                            .await;
                    }
                } else {
                    self.fallback_video(&delivery.streamer);
                }
            }
            "StreamViewerSfuFallback" | "StreamViewerSfuReady" => {
                if let (Some(peer), Some(epoch)) = (
                    args.first().and_then(Value::as_str),
                    args.get(1).and_then(Value::as_u64),
                ) {
                    if self.peers.contains(peer) {
                        self.engine.viewer_sfu_ready(
                            peer,
                            epoch,
                            target == "StreamViewerSfuFallback",
                        )?;
                    }
                }
            }
            "UserStartedStreaming" => {
                if let Some(peer) = args
                    .first()
                    .and_then(Value::as_str)
                    .filter(|peer| self.peers.contains(*peer))
                {
                    self.streams.streaming.insert(peer.into());
                }
                self.report_stream_interest(hub).await;
            }
            "UserStoppedStreaming" | "StreamDropped" => {
                if let Some(peer) = args.first().and_then(Value::as_str) {
                    self.streams.streaming.remove(peer);
                    self.streams.snapshots.remove(peer);
                    self.streams.probes.remove(peer);
                    self.streams.probe_deadlines.remove(peer);
                    self.streams.selected.remove(peer);
                    self.streams.reported.remove(peer);
                    self.streams.p2p_video.remove(peer);
                    self.streams.p2p_audio.remove(peer);
                    self.streams.sfu_audio.remove(peer);
                    if let Some(playback) = self.playback.as_mut() {
                        playback.remove_stream(peer);
                    }
                    self.engine.close_sfu_subscriber(peer);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
