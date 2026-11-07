#![cfg(feature = "native-media")]

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use zabor_native::audio_mix::AudioControls;
use zabor_native::audio_runtime::{SharedAudio, SharedAudioDevice};
use zabor_native::audio_settings::AudioSettings;
use zabor_native::mic_test::{
    MicTestIssue, MicTestStage, CLIP_SAMPLES, MONITOR_SETTLE, PIPELINE_FLUSH,
};
use zabor_native::mic_test_ui::MicTestUi;

struct Device(CancellationToken);

impl SharedAudioDevice for Device {
    fn failed(&self) -> bool {
        self.0.is_cancelled()
    }
    fn cancel(&self) {
        self.0.cancel();
    }
}

fn shared(permits: usize) -> (Arc<SharedAudio<Device>>, Arc<Semaphore>, Arc<AtomicUsize>) {
    let gate = Arc::new(Semaphore::new(permits));
    let starts = Arc::new(AtomicUsize::new(0));
    let factory_gate = gate.clone();
    let factory_starts = starts.clone();
    let shared = SharedAudio::new(
        Arc::new(AudioControls::new(AudioSettings::default())),
        CancellationToken::new(),
        move |_, _, stop| {
            let gate = factory_gate.clone();
            let starts = factory_starts.clone();
            Box::pin(async move {
                starts.fetch_add(1, Ordering::AcqRel);
                gate.acquire().await.unwrap().forget();
                Ok(Device(stop))
            })
        },
    );
    (shared, gate, starts)
}

async fn started(starts: &AtomicUsize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while starts.load(Ordering::Acquire) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

fn complete(shared: &SharedAudio<Device>, now: Instant) {
    let token = shared.mic_test.capture_token().unwrap();
    shared.mic_test.prepared(token, now);
    shared.mic_test.snapshot(now + MONITOR_SETTLE);
    for index in 0..500 {
        shared.mic_test.capture(
            token,
            &[0.1; 480],
            -20.0,
            now + MONITOR_SETTLE + PIPELINE_FLUSH + Duration::from_millis(index * 10),
        );
    }
}

#[tokio::test]
async fn hidden_modal_owner_does_not_prepare_until_the_audio_tab_is_visible() {
    let (shared, _, starts) = shared(1);
    let mut ui = MicTestUi::new(shared.clone());
    ui.synchronize(
        "owner",
        true,
        false,
        true,
        &tokio::runtime::Handle::current(),
    );
    assert!(!shared.mic_test.has_demand());
    assert_eq!(starts.load(Ordering::Acquire), 0);
    ui.synchronize(
        "owner",
        true,
        true,
        true,
        &tokio::runtime::Handle::current(),
    );
    started(&starts).await;
    assert!(shared.mic_test.has_demand());
    assert!(ui.snapshot(Instant::now()).visible);
    assert_eq!(starts.load(Ordering::Acquire), 1);
    ui.close();
}

#[tokio::test]
async fn switching_tabs_pauses_and_retains_clip_without_late_autoplay_or_a_second_runtime() {
    let (shared, _, starts) = shared(1);
    let mut ui = MicTestUi::new(shared.clone());
    let runtime = tokio::runtime::Handle::current();
    ui.synchronize("owner", true, true, true, &runtime);
    started(&starts).await;
    let now = Instant::now();
    assert!(ui.record(now, &runtime));
    let token = shared.mic_test.capture_token().unwrap();
    shared.mic_test.prepared(token, now);
    ui.synchronize("owner", true, false, true, &runtime);
    complete(&shared, now);
    let hidden = ui.snapshot(now + Duration::from_secs(6));
    assert_eq!(hidden.stage, MicTestStage::Ready);
    assert_eq!(hidden.stored_samples, CLIP_SAMPLES);
    assert!(!hidden.playing);
    assert!(!ui.toggle_playback());
    assert!(!ui.seek(1.0));
    ui.synchronize("owner", true, true, true, &runtime);
    assert!(!ui.snapshot(now + Duration::from_secs(6)).playing);
    assert!(ui.toggle_playback());
    assert!(ui.seek(1.5));
    assert_eq!(ui.snapshot(now).position_seconds, 1.5);
    assert_eq!(starts.load(Ordering::Acquire), 1);
    ui.close();
    assert_eq!(shared.mic_test.snapshot(now).stored_samples, 0);
}

#[tokio::test]
async fn close_clears_immediately_even_while_the_async_preparation_still_owns_a_lease() {
    let (shared, gate, starts) = shared(0);
    let mut ui = MicTestUi::new(shared.clone());
    let runtime = tokio::runtime::Handle::current();
    ui.synchronize("owner", true, true, true, &runtime);
    let now = Instant::now();
    assert!(ui.record(now, &runtime));
    started(&starts).await;
    let token = shared.mic_test.capture_token().unwrap();
    ui.close();
    assert_eq!(shared.mic_test.snapshot(now).stored_samples, 0);
    assert!(!shared.mic_test.has_demand());
    assert!(!shared.mic_test.prepared(token, now));
    assert!(!ui.record(now, &runtime));
    gate.add_permits(1);
    shared.reset();
}

#[tokio::test]
async fn disable_logout_and_owner_change_clear_the_old_buffer_and_capture_generation() {
    let (shared, _, _) = shared(4);
    let runtime = tokio::runtime::Handle::current();
    let mut ui = MicTestUi::new(shared.clone());
    for (owner, open, enabled) in [
        ("owner", true, false),
        ("", false, true),
        ("another-owner", true, true),
    ] {
        ui.synchronize("owner", true, true, true, &runtime);
        let now = Instant::now();
        assert!(ui.record(now, &runtime));
        let old = shared.mic_test.capture_token().unwrap();
        ui.synchronize(owner, open, true, enabled, &runtime);
        assert_eq!(shared.mic_test.snapshot(now).stored_samples, 0);
        assert!(!shared.mic_test.prepared(old, now));
        assert!(!shared.mic_test.capture(old, &[0.1; 480], -20.0, now));
        ui.close();
    }
}

#[tokio::test]
async fn app_reset_invalidates_ui_actions_and_cannot_resurrect_the_previous_clip() {
    let (shared, _, _) = shared(1);
    let runtime = tokio::runtime::Handle::current();
    let mut ui = MicTestUi::new(shared.clone());
    ui.synchronize("owner", true, true, true, &runtime);
    let now = Instant::now();
    assert!(ui.record(now, &runtime));
    complete(&shared, now);
    shared.reset();
    assert!(!ui.toggle_playback());
    assert!(!ui.seek(1.0));
    assert!(!ui.record(now, &runtime));
    assert_eq!(ui.snapshot(now).stored_samples, 0);
    ui.synchronize("owner", true, true, true, &runtime);
    assert!(!ui.record(now, &runtime));
    assert!(!shared.mic_test.has_demand());
    ui.synchronize("new-owner", true, true, true, &runtime);
    assert_eq!(ui.snapshot(now).stage, MicTestStage::Idle);
    ui.close();
}

#[tokio::test]
async fn failed_preparation_exposes_an_error_and_retaking_can_retry_without_network_commands() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    let shared = SharedAudio::new(
        Arc::new(AudioControls::new(AudioSettings::default())),
        CancellationToken::new(),
        move |_, _, stop| {
            let attempts = attempts.clone();
            Box::pin(async move {
                if attempts.fetch_add(1, Ordering::AcqRel) == 0 {
                    Err(zabor_native::media::MediaError::Microphone)
                } else {
                    Ok(Device(stop))
                }
            })
        },
    );
    let runtime = tokio::runtime::Handle::current();
    let mut ui = MicTestUi::new(shared.clone());
    ui.synchronize("owner", true, true, true, &runtime);
    let now = Instant::now();
    started(&observed).await;
    tokio::task::yield_now().await;
    assert_eq!(ui.snapshot(now).issue, Some(MicTestIssue::Failed));
    assert!(ui.record(now, &runtime));
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    assert_eq!(observed.load(Ordering::Acquire), 2);
    assert_eq!(ui.snapshot(now).stage, MicTestStage::Arming);
    assert_eq!(ui.snapshot(now).issue, None);
    complete(&shared, now);
    assert_eq!(
        ui.snapshot(now + Duration::from_secs(6)).stage,
        MicTestStage::Ready
    );
    ui.close();
}

#[tokio::test]
async fn duplicate_record_clicks_are_ignored_but_ready_retakes_revoke_the_old_clip() {
    let (shared, _, _) = shared(1);
    let runtime = tokio::runtime::Handle::current();
    let mut ui = MicTestUi::new(shared.clone());
    ui.synchronize("owner", true, true, true, &runtime);
    let now = Instant::now();
    assert!(ui.record(now, &runtime));
    let token = shared.mic_test.capture_token().unwrap();
    assert!(!ui.record(now, &runtime));
    assert_eq!(shared.mic_test.capture_token(), Some(token));
    complete(&shared, now);
    assert!(ui.record(now + Duration::from_secs(6), &runtime));
    assert_ne!(shared.mic_test.capture_token(), Some(token));
    assert_eq!(
        ui.snapshot(now + Duration::from_secs(6)).duration_seconds,
        0.0
    );
    ui.close();
}
