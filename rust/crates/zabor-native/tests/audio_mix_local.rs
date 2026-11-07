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
