#![cfg(feature = "native-media")]

use libwebrtc::media_stream_track::MediaStreamTrack;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use zabor_native::audio_mix::{AudioControls, AudioMixer};
use zabor_native::audio_playback::AudioPlayback;
use zabor_native::audio_settings::AudioSettings;
use zabor_native::media::{MediaEngine, MediaEvent};
use zabor_native::microphone::{audio_source, MicProcessor, MicSubmission};

struct CaptureFixture {
    stop: CancellationToken,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for CaptureFixture {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

#[tokio::test]
async fn processed_microphone_reaches_bounded_playback_over_real_local_webrtc_and_restarts_safely()
{
    tokio::time::timeout(Duration::from_secs(20), async {
        let (mut alice, mut alice_events) = MediaEngine::new("alice".into());
        let (mut bob, mut bob_events) = MediaEngine::new("bob".into());
        alice.authorize_peers(["bob".into()]);
        bob.authorize_peers(["alice".into()]);
        let source = audio_source();
        alice.attach_audio_source(source.clone()).unwrap();
        let offer = alice.offer("bob", false).await.unwrap();
        let answer = bob.receive_offer("alice", &offer).await.unwrap().unwrap();
        alice.receive_answer("bob", &answer).await.unwrap();
        let (track, lifetime) = loop {
            tokio::select! {
                event = alice_events.recv() => { if let Some(MediaEvent::Ice { payload, .. }) = event { bob.receive_ice("alice", &payload).await.unwrap(); } },
                event = bob_events.recv() => match event {
                    Some(MediaEvent::Ice { payload, .. }) => alice.receive_ice("bob", &payload).await.unwrap(),
                    Some(MediaEvent::Track { track: MediaStreamTrack::Audio(track), lifetime, .. }) => break (track, lifetime),
                    _ => {},
                },
            }
        };
        let controls = Arc::new(AudioControls::new(AudioSettings { noise_suppression: false, ..AudioSettings::default() }));
        let capture_mix = Arc::new(AudioMixer::new(controls.clone()));
        let stop = CancellationToken::new();
        let worker_stop = stop.clone();
        let worker_controls = controls.clone();
        let thread = std::thread::Builder::new().stack_size(16 * 1024 * 1024).spawn(move || {
            let mut mic = MicProcessor::new(worker_controls.clone(), capture_mix).unwrap();
            let submit = MicSubmission::new(source, worker_controls, worker_stop.clone());
            let mut block = 0;
            while !worker_stop.is_cancelled() {
                let input: [f32; 480] = std::array::from_fn(|index| ((block * 480 + index) as f64 * std::f64::consts::TAU * 440.0 / 48000.0).sin() as f32 * 0.1);
                let mut pcm = mic.process(&input, 1, false);
                submit.submit(&mut pcm, mic.epoch()).unwrap();
                block += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
        }).unwrap();
        let _capture = CaptureFixture { stop, thread: Some(thread) };
        let output_controls = Arc::new(AudioControls::new(AudioSettings::default()));
        let mixer = Arc::new(AudioMixer::new(output_controls.clone()));
        let mut playback = AudioPlayback::new(mixer.clone());
        let mut ticker = tokio::time::interval(Duration::from_millis(10));
        for _ in 0..3 {
            playback.attach("alice", track.clone(), lifetime.clone(), false).unwrap();
            assert_eq!(playback.subscription_count(), 1);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let mut pcm = [0.0; 960];
                        mixer.render_pcm(&mut pcm);
                        if pcm.iter().any(|sample| sample.abs() > 0.01) { break; }
                    },
                    event = alice_events.recv() => { if let Some(MediaEvent::Ice { payload, .. }) = event { bob.receive_ice("alice", &payload).await.unwrap(); } },
                    event = bob_events.recv() => { if let Some(MediaEvent::Ice { payload, .. }) = event { alice.receive_ice("bob", &payload).await.unwrap(); } },
                }
            }
        }
        output_controls.set_user_state(false, true);
        let mut pcm = [1.0; 960];
        mixer.render_pcm(&mut pcm);
        assert_eq!(pcm, [0.0; 960]);
        bob.disconnect_peer("alice");
        assert!(!lifetime.is_alive());
        playback.retain_alive();
        assert_eq!(playback.subscription_count(), 0);
        output_controls.set_user_state(false, false);
        mixer.render_pcm(&mut pcm);
        assert_eq!(pcm, [0.0; 960]);
        playback.clear();
        alice.close();
        bob.close();
    }).await.unwrap();
}
