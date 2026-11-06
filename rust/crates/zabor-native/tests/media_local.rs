#![cfg(feature = "native-media")]

use futures_util::StreamExt;
use libwebrtc::audio_frame::AudioFrame;
use libwebrtc::audio_source::{native::NativeAudioSource, AudioSourceOptions};
use libwebrtc::audio_stream::native::NativeAudioStream;
use libwebrtc::data_channel::{DataChannelInit, DataChannelState};
use libwebrtc::media_stream_track::MediaStreamTrack;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::timeout;
use zabor_native::media::{MediaEngine, MediaError, MediaEvent};

#[tokio::test]
async fn native_peers_exchange_sctp_data_without_a_server_or_microphone() {
    timeout(Duration::from_secs(20), async {
        let (mut alice, mut alice_events) = MediaEngine::new("alice".into());
        let (mut bob, mut bob_events) = MediaEngine::new("bob".into());
        assert!(matches!(alice.offer("uninvited", false).await, Err(MediaError::UnauthorizedPeer)));
        alice.authorize_peers(["bob".into()]);
        bob.authorize_peers(["alice".into()]);
        let source = NativeAudioSource::new(AudioSourceOptions { echo_cancellation: false, noise_suppression: false, auto_gain_control: false }, 48000, 1, 20);
        alice.attach_audio_source(source.clone()).unwrap();
        let channel = alice.create_data_channel("bob", "local-fixture", DataChannelInit::default()).unwrap();
        let opened = Arc::new(Notify::new());
        let opened_callback = opened.clone();
        channel.on_state_change(Some(Box::new(move |state| { if state == DataChannelState::Open { opened_callback.notify_one(); } })));
        let offer = alice.offer("bob", false).await.unwrap();
        let answer = bob.receive_offer("alice", &offer).await.unwrap().unwrap();
        alice.receive_answer("bob", &answer).await.unwrap();
        let mut remote = None;
        let mut audio = None;
        while channel.state() != DataChannelState::Open || remote.is_none() || audio.is_none() {
            tokio::select! {
                _ = opened.notified() => {},
                event = alice_events.recv() => { if let Some(MediaEvent::Ice { payload, .. }) = event { bob.receive_ice("alice", &payload).await.unwrap(); } },
                event = bob_events.recv() => { match event {
                    Some(MediaEvent::Ice { payload, .. }) => alice.receive_ice("bob", &payload).await.unwrap(),
                    Some(MediaEvent::DataChannel { channel, .. }) => remote = Some(channel),
                    Some(MediaEvent::Track { track: MediaStreamTrack::Audio(track), .. }) => audio = Some(NativeAudioStream::new(track, 48000, 1)),
                    _ => {},
                } },
            }
        }
        let remote = remote.unwrap();
        let received = Arc::new(Notify::new());
        let notify = received.clone();
        remote.on_message(Some(Box::new(move |buffer| { if buffer.binary && buffer.data == b"native-local-roundtrip" { notify.notify_one(); } })));
        channel.send(b"native-local-roundtrip", true).unwrap();
        received.notified().await;
        let producer = tokio::spawn(async move {
            let mut frame = AudioFrame::new(48000, 1, 480);
            for block in 0..100 {
                for (index, sample) in frame.data.to_mut().iter_mut().enumerate() { *sample = (((block * 480 + index) as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin() * 8000.0) as i16; }
                source.capture_frame(&frame).await.unwrap();
            }
        });
        let mut audio = audio.unwrap();
        let mut decoded = 0;
        while decoded < 10 {
            let frame = audio.next().await.unwrap();
            assert_eq!(frame.sample_rate, 48000);
            assert_eq!(frame.num_channels, 1);
            if frame.data.iter().any(|sample| sample.unsigned_abs() > 500) { decoded += 1; }
        }
        producer.abort();
        let _ = producer.await;
        audio.close();
        remote.on_message(None);
        remote.close();
        channel.close();
        channel.on_state_change(None);
        alice.close();
        bob.close();
        assert!(matches!(alice.offer("bob", false).await, Err(MediaError::UnauthorizedPeer)));
    }).await.unwrap();
}
