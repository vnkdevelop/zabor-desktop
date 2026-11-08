#![cfg(feature = "native-media")]

use std::sync::Arc;
use zabor_native::audio_mix::{AudioControls, AudioMixer};
use zabor_native::audio_settings::AudioSettings;
use zabor_native::media::{MediaEngine, PeerLifetime};

async fn peer() -> (MediaEngine, PeerLifetime) {
    let (mut engine, _events) = MediaEngine::new_data_only("local".into());
    engine.authorize_peers(["peer/with/separator".into()]);
    engine.offer("peer/with/separator", false).await.unwrap();
    let lifetime = engine.peer_lifetime("peer/with/separator").unwrap();
    (engine, lifetime)
}

#[tokio::test]
async fn playback_gains_use_actual_peer_ids_and_never_exceed_the_output_ceiling() {
    let (_engine, lifetime) = peer().await;
    let controls = Arc::new(AudioControls::new(AudioSettings::default()));
    let mixer = AudioMixer::new(controls);
    let lane = mixer
        .add("lane".into(), "peer/with/separator", lifetime, false)
        .unwrap();
    let mut settings = AudioSettings {
        output_volume: 50.0,
        ..AudioSettings::default()
    };
    settings
        .user_volumes
        .insert("peer/with/separator".into(), 50.0);
    mixer.update_gains(&settings);
    lane.push(&[16384; 480]);
    let mut stereo = [0.0; 960];
    mixer.render(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.125));
    settings.output_volume = 200.0;
    settings
        .user_volumes
        .insert("peer/with/separator".into(), 200.0);
    mixer.update_gains(&settings);
    lane.push(&[32767; 480]);
    mixer.render(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.84139514));
}

#[tokio::test]
async fn deafen_and_stream_focus_discard_backlogs_instead_of_replaying_them() {
    let (_engine, lifetime) = peer().await;
    let controls = Arc::new(AudioControls::new(AudioSettings::default()));
    let mixer = AudioMixer::new(controls.clone());
    let voice = mixer
        .add(
            "voice".into(),
            "peer/with/separator",
            lifetime.clone(),
            false,
        )
        .unwrap();
    let stream = mixer
        .add("stream".into(), "peer/with/separator", lifetime, true)
        .unwrap();
    voice.push(&[4096; 9600]);
    stream.push(&[4096; 9600]);
    controls.set_user_state(false, true);
    let mut stereo = [1.0; 960];
    mixer.render(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.0));
    controls.set_user_state(false, false);
    mixer.focus(Some("peer/with/separator".into()));
    mixer.render(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.0));
    voice.push(&[4096; 480]);
    stream.push(&[4096; 480]);
    mixer.render(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.25));
    mixer.focus(None);
    stream.push(&[4096; 9600]);
    mixer.render(&mut stereo);
    mixer.focus(Some("peer/with/separator".into()));
    mixer.render(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.0));
}

#[tokio::test]
async fn revoked_and_replaced_lanes_cannot_play_queued_callbacks() {
    let (mut engine, lifetime) = peer().await;
    let mixer = AudioMixer::new(Arc::new(AudioControls::new(AudioSettings::default())));
    let old = mixer
        .add(
            "voice".into(),
            "peer/with/separator",
            lifetime.clone(),
            false,
        )
        .unwrap();
    let current = mixer
        .add("voice".into(), "peer/with/separator", lifetime, false)
        .unwrap();
    old.push(&[16384; 480]);
    let mut stereo = [1.0; 960];
    mixer.render(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.0));
    current.push(&[16384; 480]);
    engine.disconnect_peer("peer/with/separator");
    current.push(&[16384; 480]);
    mixer.render(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.0));
}

#[tokio::test]
async fn playback_and_aec_reverse_queues_are_bounded() {
    let (_engine, lifetime) = peer().await;
    let mixer = AudioMixer::new(Arc::new(AudioControls::new(AudioSettings::default())));
    let lane = mixer
        .add("voice".into(), "peer/with/separator", lifetime, false)
        .unwrap();
    for _ in 0..40 {
        lane.push(&[4096; 480]);
    }
    let mut stereo = [0.0; 19200];
    mixer.render(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.125));
    assert_eq!(mixer.reverse.lock().unwrap().len(), 4800);
    mixer.render(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.0));
    assert_eq!(mixer.reverse.lock().unwrap().len(), 4800);
}

#[tokio::test]
async fn focused_stream_pcm_preserves_stereo_and_local_mute_does_not_change_remote_music() {
    let (_engine, lifetime) = peer().await;
    let controls = Arc::new(AudioControls::new(AudioSettings::default()));
    let mixer = AudioMixer::new(controls.clone());
    let voice = mixer
        .add(
            "voice".into(),
            "peer/with/separator",
            lifetime.clone(),
            false,
        )
        .unwrap();
    let stream = mixer
        .add("stereo".into(), "peer/with/separator", lifetime, true)
        .unwrap();
    mixer.focus(Some("peer/with/separator".into()));
    controls.set_user_state(true, false);
    voice.push(&[4096; 480]);
    let pcm: Vec<i16> = (0..480).flat_map(|_| [-8192, 16384]).collect();
    stream.push_stereo(&pcm);
    let mut stereo = [0.0; 960];
    mixer.render_pcm(&mut stereo);
    assert!(stereo
        .as_chunks::<2>()
        .0
        .iter()
        .all(|pair| *pair == [-0.125, 0.625]));
    stream.push_stereo(&[123; 3]);
    mixer.render_pcm(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.0));
    stream.push_stereo(&pcm);
    mixer.focus(None);
    mixer.render_pcm(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.0));
    mixer.focus(Some("peer/with/separator".into()));
    mixer.render_pcm(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.0));
}

#[tokio::test]
async fn stream_stereo_queue_is_frame_aligned_and_independent_of_voice_queue_capacity() {
    let (_engine, lifetime) = peer().await;
    let mixer = AudioMixer::new(Arc::new(AudioControls::new(AudioSettings::default())));
    let voice = mixer
        .add(
            "voice".into(),
            "peer/with/separator",
            lifetime.clone(),
            false,
        )
        .unwrap();
    let stream = mixer
        .add("stereo".into(), "peer/with/separator", lifetime, true)
        .unwrap();
    mixer.focus(Some("peer/with/separator".into()));
    let pcm: Vec<i16> = (0..9600).flat_map(|_| [-4096, 4096]).collect();
    for _ in 0..3 {
        stream.push_stereo(&pcm);
    }
    voice.push_stereo(&pcm);
    let mut stereo = [0.0; 19200];
    mixer.render_pcm(&mut stereo);
    assert!(stereo
        .as_chunks::<2>()
        .0
        .iter()
        .all(|pair| *pair == [-0.125, 0.125]));
    mixer.render_pcm(&mut stereo);
    assert!(stereo.iter().all(|sample| *sample == 0.0));
}

#[tokio::test]
async fn short_deafen_toggles_and_old_subscription_cleanup_cannot_replay_or_remove_new_audio() {
    let (_engine, lifetime) = peer().await;
    let controls = Arc::new(AudioControls::new(AudioSettings::default()));
    let mixer = AudioMixer::new(controls.clone());
    let old = mixer
        .add(
            "voice".into(),
            "peer/with/separator",
            lifetime.clone(),
            false,
        )
        .unwrap();
    old.push(&[16384; 480]);
    controls.set_user_state(false, true);
    controls.set_user_state(false, false);
    let mut output = [1.0; 960];
    mixer.render_pcm(&mut output);
    assert_eq!(output, [0.0; 960]);
    old.push(&[16384; 480]);
    controls.set_server_state(false, true);
    controls.set_server_state(false, false);
    mixer.render_pcm(&mut output);
    assert_eq!(output, [0.0; 960]);
    let current = mixer
        .add("voice".into(), "peer/with/separator", lifetime, false)
        .unwrap();
    mixer.remove_lane("voice", &old);
    current.push(&[16384; 480]);
    mixer.render_pcm(&mut output);
    assert_eq!(output, [0.5; 960]);
    assert!(mixer.reverse.lock().unwrap().is_empty());
    mixer.played(&output);
    assert_eq!(mixer.reverse.lock().unwrap().len(), 480);
}

#[tokio::test]
async fn output_restart_preserves_peer_lanes_but_clears_stale_pcm_and_aec_feedback() {
    let (_engine, lifetime) = peer().await;
    let controls = Arc::new(AudioControls::new(AudioSettings::default()));
    let mixer = AudioMixer::new(controls.clone());
    let lane = mixer
        .add("voice".into(), "peer/with/separator", lifetime, false)
        .unwrap();
    lane.push(&[16384; 480]);
    mixer.played(&[0.5; 960]);
    let epoch = controls
        .capture_epoch
        .load(std::sync::atomic::Ordering::Acquire);
    mixer.restart_output();
    assert!(lane.is_active());
    assert!(mixer.reverse.lock().unwrap().is_empty());
    assert_ne!(
        controls
            .capture_epoch
            .load(std::sync::atomic::Ordering::Acquire),
        epoch
    );
    let mut output = [1.0; 960];
    mixer.render_pcm(&mut output);
    assert_eq!(output, [0.0; 960]);
    lane.push(&[16384; 480]);
    mixer.render_pcm(&mut output);
    assert_eq!(output, [0.5; 960]);
}
