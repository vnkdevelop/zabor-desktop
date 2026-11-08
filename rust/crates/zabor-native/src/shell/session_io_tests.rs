use super::{Command, CommandQueue, SessionIo, SessionWork, UiEvent, VoiceLeave};
use std::collections::{BTreeMap, BTreeSet};
use std::future::{poll_fn, Future};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Notify};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use zabor_native::audio_preferences::AudioPreferences;
use zabor_native::audio_runtime::SharedAudio;
use zabor_native::auth::Credentials;
use zabor_native::call_recovery::CallRecovery;
use zabor_native::critical_intents::{CriticalAction, CriticalIntents, IntentTicket};
use zabor_native::native_audio::NativeAudio;
use zabor_native::profile::Profile;
use zabor_native::video::StreamDropGrace;
use zeroize::Zeroizing;

fn credentials() -> Credentials {
    Credentials {
        username: "fixture".into(),
        password: Zeroizing::new("fixture-only".into()),
    }
}

async fn resource_holder(
    profile: Arc<Profile>,
) -> (std::sync::mpsc::Sender<()>, tokio::task::JoinHandle<()>) {
    let prepared = profile
        .prepare_session("resource-holder", &credentials())
        .unwrap();
    let held = Arc::new(Notify::new());
    let (release, blocked) = std::sync::mpsc::channel();
    let entered = held.clone();
    let job = tokio::task::spawn_blocking(move || {
        assert!(!profile
            .commit_session_if(
                prepared,
                || {
                    entered.notify_one();
                    blocked.recv().unwrap();
                    false
                },
                || false
            )
            .unwrap());
    });
    held.notified().await;
    (release, job)
}

async fn started<T>(future: impl Future<Output = T>, notify: &Notify) -> T {
    tokio::pin!(future);
    let mut first = true;
    poll_fn(|cx| {
        let result = future.as_mut().poll(cx);
        if first {
            first = false;
            notify.notify_one();
        }
        result
    })
    .await
}

async fn drive<T>(
    work: &SessionWork<'_>,
    ticket: IntentTicket,
    audio: &SharedAudio<NativeAudio>,
    generation: &AtomicU64,
    future: impl Future<Output = T>,
) -> Option<T> {
    let mut voice = None;
    let mut events = None;
    let mut voice_ticket = Some(ticket);
    let mut pending_join = Some("old-channel".into());
    let mut videos = BTreeMap::new();
    let mut paused = BTreeSet::new();
    let mut drops = StreamDropGrace::default();
    let mut recovery = CallRecovery::default();
    let mut abandoned = false;
    let mut stream = false;
    let mut connected = true;
    let mut sent_audio = None;
    let mut sent_speaking = false;
    let mut release = None;
    let mut speaking = false;
    work.run_with_leave(
        future,
        &mut VoiceLeave {
            voice: &mut voice,
            media_events: &mut events,
            voice_ticket: &mut voice_ticket,
            pending_join: &mut pending_join,
            video_jobs: &mut videos,
            paused_video: &mut paused,
            stream_drops: &mut drops,
            call_recovery: &mut recovery,
            abandoned_call: &mut abandoned,
            pending_stop_stream: &mut stream,
            call_connected: &mut connected,
            sent_audio_state: &mut sent_audio,
            sent_speaking: &mut sent_speaking,
            speaking_release: &mut release,
            local_speaking: &mut speaking,
            video_generation: generation,
            audio_demand: &audio.demand,
        },
        None,
    )
    .await
}

fn audio(stop: &CancellationToken) -> Arc<SharedAudio<NativeAudio>> {
    let preferences = AudioPreferences::new(&serde_json::json!({}));
    let audio = SharedAudio::new(
        preferences.controls.clone(),
        stop.child_token(),
        |_, _, _| Box::pin(async { panic!("Session I/O fixture must not start physical audio") }),
    );
    let lease = audio.demand.reserve().unwrap();
    assert!(audio.demand.activate(lease));
    audio
}

#[tokio::test]
async fn blocked_profile_resource_services_repeated_leave_and_preserves_single_save_and_monitor() {
    timeout(Duration::from_secs(10), async {
        let root = tempfile::tempdir().unwrap(); let profile = Arc::new(Profile::open(root.path()).unwrap());
        profile.save_session("original-owner", &credentials()).unwrap();
        let (release, holder) = resource_holder(profile.clone()).await;
        let intents = Arc::new(CriticalIntents::default()); let ticket = intents.ticket(); let stop = CancellationToken::new();
        let io = SessionIo::new(Some(profile.clone()), intents.clone(), stop.clone()); let audio = audio(&stop); let monitor = audio.monitor(true);
        let generation = Arc::new(AtomicU64::new(1)); let prepared = io.prepare("accepted-owner".into(), credentials()).await.unwrap();
        let waiting = Arc::new(Notify::new()); let (updates, mut published) = mpsc::unbounded_channel();
        let (sender, mut commands) = mpsc::channel(16); let queue = CommandQueue { sender, intents: intents.clone() };
        for _ in 0..16 { queue.try_send(Command::Join("old-channel".into())).unwrap_or_else(|_| panic!("Voice queue fixture failed")); }
        let task = {
            let intents = intents.clone(); let io = io.clone(); let audio = audio.clone(); let generation = generation.clone(); let waiting = waiting.clone(); let stop = stop.clone();
            tokio::spawn(async move {
                let update = |event| { let _ = updates.send(event); }; let work = SessionWork::new(&intents, ticket, &stop, &update);
                drive(&work, ticket, &audio, &generation, started(io.save(ticket, prepared), &waiting)).await
            })
        };
        waiting.notified().await; let measured = Instant::now();
        queue.try_send(Command::Leave).unwrap_or_else(|_| panic!("Priority Leave failed"));
        assert!(matches!(timeout(Duration::from_millis(1500), published.recv()).await.unwrap().unwrap().for_current(&intents), Some(UiEvent::VoiceEnded)));
        assert_eq!(audio.demand.active(), 0); assert_eq!(generation.load(Ordering::Acquire), 2); assert!(!task.is_finished());
        monitor.configure(false); monitor.configure(true);
        queue.try_send(Command::Leave).unwrap_or_else(|_| panic!("Repeated Leave failed"));
        assert!(matches!(timeout(Duration::from_millis(1500), published.recv()).await.unwrap().unwrap().for_current(&intents), Some(UiEvent::VoiceEnded)));
        assert!(!task.is_finished());
        release.send(()).unwrap(); holder.await.unwrap();
        assert!(matches!(task.await.unwrap(), Some(Ok(true))));
        for _ in 0..16 { let queued = commands.try_recv().unwrap(); assert!(!intents.is_current(queued.ticket, queued.command.scope())); }
        io.finish().await.unwrap();
        assert_eq!(profile.load_session().unwrap().unwrap().user_id, "accepted-owner");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
        println!("session filesystem held=resource-lock leave_ms={} saves=1 queued_voice_commands=16 repeats=1 physical_audio=false", measured.elapsed().as_millis());
    }).await.unwrap();
}

#[tokio::test]
async fn logout_cancels_blocked_save_before_filesystem_acceptance_and_next_owner_wins() {
    timeout(Duration::from_secs(10), async {
        let root = tempfile::tempdir().unwrap(); let profile = Arc::new(Profile::open(root.path()).unwrap());
        profile.save_session("original-owner", &credentials()).unwrap();
        let (release, holder) = resource_holder(profile.clone()).await;
        let intents = Arc::new(CriticalIntents::default()); let ticket = intents.ticket(); let stop = CancellationToken::new();
        let io = SessionIo::new(Some(profile.clone()), intents.clone(), stop.clone());
        let waiting = Arc::new(Notify::new()); let prepared = profile.prepare_session("revoked-owner", &credentials()).unwrap();
        let task = {
            let io = io.clone(); let intents = intents.clone(); let waiting = waiting.clone(); let stop = stop.clone();
            tokio::spawn(async move {
                let update = |_| {}; let work = SessionWork::new(&intents, ticket, &stop, &update);
                work.run(started(io.save(ticket, prepared), &waiting)).await
            })
        };
        waiting.notified().await; let measured = Instant::now();
        intents.request(CriticalAction::Logout);
        assert!(timeout(Duration::from_millis(1500), task).await.unwrap().unwrap().is_none());
        assert_eq!(intents.next().await, CriticalAction::Logout);
        let next_ticket = intents.ticket(); let next = profile.prepare_session("next-owner", &credentials()).unwrap();
        let saving_next = { let io = io.clone(); tokio::spawn(async move { io.save(next_ticket, next).await }) };
        release.send(()).unwrap(); holder.await.unwrap();
        assert!(saving_next.await.unwrap().unwrap()); io.finish().await.unwrap();
        assert_eq!(profile.load_session().unwrap().unwrap().user_id, "next-owner");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
        println!("session filesystem held=resource-lock logout_ms={} old_result=false next_owner=true physical_audio=false", measured.elapsed().as_millis());
    }).await.unwrap();
}

#[tokio::test]
async fn cancelled_window_save_is_drained_and_preserves_original_profile_without_detached_commit() {
    timeout(Duration::from_secs(10), async {
        let root = tempfile::tempdir().unwrap();
        let profile = Arc::new(Profile::open(root.path()).unwrap());
        profile
            .save_session("original-owner", &credentials())
            .unwrap();
        let previous = std::fs::read(root.path().join("session.enc")).unwrap();
        let (release, holder) = resource_holder(profile.clone()).await;
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let io = SessionIo::new(Some(profile.clone()), intents.clone(), stop.clone());
        let waiting = Arc::new(Notify::new());
        let prepared = profile
            .prepare_session("closed-window-owner", &credentials())
            .unwrap();
        let task = {
            let io = io.clone();
            let intents = intents.clone();
            let stop = stop.clone();
            let waiting = waiting.clone();
            tokio::spawn(async move {
                let update = |_| {};
                let work = SessionWork::new(&intents, ticket, &stop, &update);
                work.run(started(io.save(ticket, prepared), &waiting)).await
            })
        };
        waiting.notified().await;
        stop.cancel();
        assert!(timeout(Duration::from_millis(1500), task)
            .await
            .unwrap()
            .unwrap()
            .is_none());
        let finishing = {
            let io = io.clone();
            tokio::spawn(async move { io.finish().await })
        };
        assert!(!finishing.is_finished());
        release.send(()).unwrap();
        holder.await.unwrap();
        finishing.await.unwrap().unwrap();
        assert_eq!(
            std::fs::read(root.path().join("session.enc")).unwrap(),
            previous
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn stale_delayed_clear_cannot_delete_next_owner_and_explicit_logout_clear_drains_on_close() {
    timeout(Duration::from_secs(10), async {
        let root = tempfile::tempdir().unwrap();
        let profile = Arc::new(Profile::open(root.path()).unwrap());
        profile
            .save_session("original-owner", &credentials())
            .unwrap();
        profile
            .write_settings(&serde_json::json!({"language":"ru"}))
            .unwrap();
        let (release, holder) = resource_holder(profile.clone()).await;
        let intents = Arc::new(CriticalIntents::default());
        let old_ticket = intents.ticket();
        let stop = CancellationToken::new();
        let io = SessionIo::new(Some(profile.clone()), intents.clone(), stop.clone());
        let waiting = Arc::new(Notify::new());
        let clearing_old = {
            let io = io.clone();
            let waiting = waiting.clone();
            tokio::spawn(async move { started(io.clear(old_ticket), &waiting).await })
        };
        waiting.notified().await;
        intents.request(CriticalAction::Logout);
        assert_eq!(intents.next().await, CriticalAction::Logout);
        let new_ticket = intents.ticket();
        let next = profile
            .prepare_session("next-owner", &credentials())
            .unwrap();
        let saving_new = {
            let io = io.clone();
            tokio::spawn(async move { io.save(new_ticket, next).await })
        };
        release.send(()).unwrap();
        holder.await.unwrap();
        assert!(!clearing_old.await.unwrap().unwrap());
        assert!(saving_new.await.unwrap().unwrap());
        assert_eq!(
            profile.load_session().unwrap().unwrap().user_id,
            "next-owner"
        );
        intents.request(CriticalAction::Logout);
        assert_eq!(intents.next().await, CriticalAction::Logout);
        let logout_ticket = intents.ticket();
        let (release, holder) = resource_holder(profile.clone()).await;
        let clearing = io.clear(logout_ticket);
        stop.cancel();
        drop(clearing);
        release.send(()).unwrap();
        holder.await.unwrap();
        io.finish().await.unwrap();
        assert!(profile.load_session().unwrap().is_none());
        assert_eq!(profile.read_settings().unwrap()["language"], "ru");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn startup_read_wait_is_cancellable_and_tracked_without_republishing_saved_owner() {
    timeout(Duration::from_secs(10), async {
        let root = tempfile::tempdir().unwrap();
        let profile = Arc::new(Profile::open(root.path()).unwrap());
        profile
            .save_session("original-owner", &credentials())
            .unwrap();
        let (release, holder) = resource_holder(profile.clone()).await;
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let io = SessionIo::new(Some(profile.clone()), intents.clone(), stop.clone());
        let waiting = Arc::new(Notify::new());
        let task = {
            let io = io.clone();
            let intents = intents.clone();
            let stop = stop.clone();
            let waiting = waiting.clone();
            tokio::spawn(async move {
                let update = |_| {};
                let work = SessionWork::new(&intents, ticket, &stop, &update);
                work.run(started(io.load(), &waiting)).await
            })
        };
        waiting.notified().await;
        intents.request(CriticalAction::Logout);
        assert!(timeout(Duration::from_millis(1500), task)
            .await
            .unwrap()
            .unwrap()
            .is_none());
        release.send(()).unwrap();
        holder.await.unwrap();
        io.finish().await.unwrap();
        assert_eq!(
            profile.load_session().unwrap().unwrap().user_id,
            "original-owner"
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unreadable_startup_session_keeps_legacy_fallback_but_write_failure_remains_a_drain_error()
{
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    timeout(Duration::from_secs(10), async {
        let root = tempfile::tempdir().unwrap();
        let profile = Arc::new(Profile::open(root.path()).unwrap());
        let session_path = root.path().join("session.enc");
        std::fs::write(&session_path, b"fixture-invalid-session").unwrap();
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let io = SessionIo::new(Some(profile.clone()), intents, CancellationToken::new());
        assert!(io.load().await.is_err());
        io.finish().await.unwrap();
        let exclusive = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&session_path)
            .unwrap();
        assert!(io.clear(ticket).await.is_err());
        drop(exclusive);
        assert_eq!(
            std::fs::read(&session_path).unwrap(),
            b"fixture-invalid-session"
        );
        assert!(io.clear(ticket).await.unwrap());
        assert!(io.finish().await.is_err());
        assert!(!session_path.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    })
    .await
    .unwrap();
}
