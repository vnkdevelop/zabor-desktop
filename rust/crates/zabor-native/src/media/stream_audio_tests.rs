use super::*;
use crate::microphone::audio_source;
use libwebrtc::audio_source::AudioSourceOptions;
use tokio::time::{timeout, Duration};

fn source() -> NativeAudioSource {
    NativeAudioSource::new(
        AudioSourceOptions {
            echo_cancellation: false,
            noise_suppression: false,
            auto_gain_control: false,
        },
        48000,
        2,
        0,
    )
}

#[test]
fn remote_stream_audio_mid_classification_uses_sdp_order_not_callback_order_or_stream_label() {
    let sdp = "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:voice-first\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=mid:video\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\na=mid:files\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:music-second\r\n";
    assert_eq!(
        stream_audio_mids(sdp),
        ["music-second".into()].into_iter().collect()
    );
    assert!(stream_audio_mids("m=audio 9 UDP/TLS/RTP/SAVPF 111\na=mid:voice").is_empty());
}

#[tokio::test]
async fn stream_sender_replacement_preserves_microphone_original_peer_and_bounded_second_transceiver(
) {
    let (mut engine, _) = MediaEngine::new("owner".into());
    engine.attach_audio_source(audio_source()).unwrap();
    engine.authorize_peers(["peer".into()]);
    engine.set_stream_audio(Some(source())).unwrap();
    engine
        .create_data_channel("peer", "local-audio-owner", DataChannelInit::default())
        .unwrap();
    let lifetime = engine.peer_lifetime("peer").unwrap();
    let microphone = engine.microphone.clone().unwrap();
    let first = engine.local_stream_audio().unwrap();
    engine.set_muted(true);
    assert!(!microphone.enabled() && first.enabled());
    let count = engine.peers["peer"].connection.transceivers().len();
    for _ in 0..4 {
        engine.set_stream_audio(None).unwrap();
        assert!(!engine.has_live_stream_audio());
        assert!(!microphone.enabled());
        assert!(engine.peers["peer"]
            .stream_sender
            .as_ref()
            .unwrap()
            .track()
            .is_none());
        engine.set_stream_audio(Some(source())).unwrap();
        assert!(engine.has_live_stream_audio());
        assert!(!microphone.enabled());
        assert_eq!(engine.peers["peer"].connection.transceivers().len(), count);
        assert!(engine.peer_lifetime("peer").unwrap().is_same(&lifetime));
    }
    assert!(!first.enabled());
    let offer = engine.offer("peer", false).await.unwrap();
    let sdp: Value = serde_json::from_str(&offer).unwrap();
    assert_eq!(stream_audio_mids(sdp["sdp"].as_str().unwrap()).len(), 1);
    assert!(sdp["sdp"]
        .as_str()
        .unwrap()
        .contains("a=msid:zabor-screen "));
    let parameters = engine.peers["peer"]
        .stream_sender
        .as_ref()
        .unwrap()
        .parameters();
    assert_eq!(parameters.encodings[0].max_bitrate, Some(64_000));
    assert_eq!(parameters.encodings[0].priority, Priority::High);
    engine.close();
    assert!(!microphone.enabled() && !lifetime.is_alive());
}

#[tokio::test]
async fn local_offer_tracks_distinguish_remote_voice_from_second_stream_audio() {
    timeout(Duration::from_secs(5), async {
        let (mut sender, _) = MediaEngine::new("a".into());
        sender.attach_audio_source(audio_source()).unwrap();
        sender.set_stream_audio(Some(source())).unwrap();
        sender.authorize_peers(["b".into()]);
        let offer = sender.offer("b", false).await.unwrap();
        let (mut receiver, mut events) = MediaEngine::new("b".into());
        receiver.authorize_peers(["a".into()]);
        assert!(receiver.receive_offer("a", &offer).await.unwrap().is_some());
        let mut tracks = Vec::new();
        while tracks.len() < 2 {
            if let MediaEvent::Track {
                track: MediaStreamTrack::Audio(track),
                stream,
                ..
            } = events.recv().await.unwrap()
            {
                tracks.push((track.id(), stream));
            }
        }
        assert!(tracks.contains(&("zabor-microphone".into(), false)));
        assert_eq!(
            tracks.iter().filter(|(_, stream)| *stream).count(),
            1,
            "{tracks:?}"
        );
        assert!(tracks
            .iter()
            .any(|(id, stream)| *stream && id != "zabor-microphone"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn late_stream_audio_reuses_receive_only_slot_without_a_phantom_sender_before_start() {
    let (mut engine, _) = MediaEngine::new("owner".into());
    engine.attach_audio_source(audio_source()).unwrap();
    engine.authorize_peers(["peer".into()]);
    engine
        .create_data_channel("peer", "late-audio-fixture", DataChannelInit::default())
        .unwrap();
    let transceivers = engine.peers["peer"].connection.transceivers();
    let audio: Vec<_> = transceivers
        .iter()
        .filter(|transceiver| {
            matches!(
                transceiver.receiver().track(),
                Some(MediaStreamTrack::Audio(_))
            )
        })
        .collect();
    assert_eq!(audio.len(), 2);
    assert_eq!(audio[1].direction(), RtpTransceiverDirection::RecvOnly);
    assert!(audio[1].sender().track().is_none());
    engine.set_stream_audio(Some(source())).unwrap();
    assert_eq!(
        engine.peers["peer"].connection.transceivers().len(),
        transceivers.len()
    );
    let track = engine.local_stream_audio().unwrap();
    assert_eq!(audio[1].direction(), RtpTransceiverDirection::SendRecv);
    assert_eq!(audio[1].sender().track().unwrap().id(), track.id());
    let offer = engine.offer("peer", false).await.unwrap();
    let sdp: Value = serde_json::from_str(&offer).unwrap();
    assert_eq!(stream_audio_mids(sdp["sdp"].as_str().unwrap()).len(), 1);
    let parameters = engine.peers["peer"]
        .stream_sender
        .as_ref()
        .unwrap()
        .parameters();
    assert_eq!(parameters.encodings[0].max_bitrate, Some(64_000));
    assert_eq!(parameters.encodings[0].priority, Priority::High);
}
