use super::*;
use crate::critical_intents::CriticalAction;
use std::cell::Cell;
use zabor_input::desktop::CleanupState;

#[test]
fn original_frame_age_route_and_ack_are_fail_closed_without_restamping() {
    let ack = Instant::now() - Duration::from_secs(1);
    let decoded = ack + Duration::from_millis(50);
    let now = decoded + Duration::from_millis(750);
    assert!(frame_admitted(None, true, decoded, ack, now));
    assert!(!frame_admitted(
        None,
        true,
        decoded,
        ack,
        now + Duration::from_nanos(1)
    ));
    assert!(!frame_admitted(Some(1), true, decoded, ack, now));
    assert!(!frame_admitted(None, false, decoded, ack, now));
    assert!(!frame_admitted(
        None,
        true,
        ack - Duration::from_nanos(1),
        ack,
        now
    ));
    assert!(!frame_admitted(
        None,
        true,
        now + Duration::from_nanos(1),
        ack,
        now
    ));
    let intents = CriticalIntents::default();
    let original = intents.ticket();
    intents.request(CriticalAction::Leave);
    let replacement = intents.take_leave(original).unwrap();
    assert!(!frame_admitted(
        None,
        intents.is_current(original, IntentScope::Voice),
        decoded,
        ack,
        now
    ));
    assert_ne!(original, replacement);
}

#[test]
fn first_ack_requires_the_original_local_request_identity_and_initiation_time() {
    let now = Instant::now();
    let original = ViewerRequest {
        initiated: now - Duration::from_millis(20),
        identity: Arc::new(()),
    };
    let accepted = now - Duration::from_millis(10);
    assert!(original.accepts_ack(&original.clone(), accepted));
    let replacement = ViewerRequest {
        initiated: now - Duration::from_millis(5),
        identity: Arc::new(()),
    };
    assert!(!replacement.accepts_ack(&replacement, accepted));
    assert!(!replacement.accepts_ack(&original, now));
    assert!(!replacement.accepts_ack(&replacement, now + Duration::from_secs(1)));
    let intents = CriticalIntents::default();
    let ticket = intents.ticket();
    assert!(intents.is_current(ticket, IntentScope::Voice));
    assert_eq!(ticket, intents.ticket());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_stop_is_immediate_once_and_old_held_cleanup_cannot_stop_same_channel_replacement()
{
    use std::sync::atomic::AtomicUsize;
    let cleanup = Arc::new(AudioCleanup::default());
    let voice = CancellationToken::new();
    let occupant = Arc::new(AtomicUsize::new(1));
    let stops = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let make_request = || {
        let current = occupant.clone();
        let stops = stops.clone();
        Arc::new(RequestRetirement {
            retired: AtomicBool::new(false),
            lease: ViewerCursorLease::new(123),
            permitted: Arc::new(AtomicBool::new(true)),
            stop: voice.child_token(),
            send_stop: Box::new(move || {
                stops[current.load(Ordering::Acquire) - 1].fetch_add(1, Ordering::AcqRel);
            }),
        })
    };
    let original = make_request();
    let on_exit = original.clone();
    let (entered, restoring) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let actor = CursorActor::start_with(
        cleanup.clone(),
        original.lease.clone(),
        original.stop.clone(),
        move |_, stop, observation| {
            while !stop.is_cancelled() {
                std::thread::park_timeout(Duration::from_millis(1));
            }
            zabor_input::desktop::run_viewer_cursor_with_retirement(
                ViewerCursorLease::new(0),
                || true,
                observation,
                || on_exit.retire(),
            )?;
            entered.send(()).unwrap();
            held.recv_timeout(Duration::from_secs(3)).unwrap();
            on_exit.retire();
            Ok(())
        },
    )
    .unwrap();
    let mut completion = actor.completion();
    original.retire();
    assert_eq!(stops[0].load(Ordering::Acquire), 1);
    assert!(!original.permitted.load(Ordering::Acquire));
    assert!(!voice.is_cancelled());
    restoring.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), cleanup.wait())
            .await
            .is_err()
    );
    assert_eq!(completion.cleanup(), CleanupState::Pending);
    occupant.store(2, Ordering::Release);
    let replacement = make_request();
    original.retire();
    drop(actor);
    release.send(()).unwrap();
    cleanup.wait().await.unwrap();
    completion.wait().await.unwrap();
    assert_eq!(stops[0].load(Ordering::Acquire), 1);
    assert_eq!(stops[1].load(Ordering::Acquire), 0);
    assert!(!replacement.stop.is_cancelled());
    replacement.retire();
    replacement.retire();
    assert_eq!(stops[1].load(Ordering::Acquire), 1);
}

struct HeldPillDrop {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    destroyed: Arc<AtomicBool>,
}

impl Drop for HeldPillDrop {
    fn drop(&mut self) {
        self.entered.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(3)).unwrap();
        self.destroyed.store(true, Ordering::Release);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_attachment_receipt_waits_for_the_original_pill_drop_and_keeps_cleanup_failure() {
    for error in [InputError::Unavailable, InputError::Cleanup] {
        let cleanup = Arc::new(AudioCleanup::default());
        let (done, mut completion) = register_surface(cleanup.clone()).unwrap();
        let observation = completion.observation();
        zabor_input::desktop::run_viewer_cursor(
            ViewerCursorLease::new(0),
            || true,
            observation.clone(),
        )
        .unwrap();
        let (entered, dropping) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let destroyed = Arc::new(AtomicBool::new(false));
        let pill = HeldPillDrop {
            entered,
            release: held,
            destroyed: destroyed.clone(),
        };
        let original =
            tokio::task::spawn_blocking(move || failed_attachment(pill, done, error, &observation));
        dropping.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!destroyed.load(Ordering::Acquire));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), cleanup.wait())
                .await
                .is_err()
        );
        assert_eq!(completion.cleanup(), CleanupState::Pending);
        release.send(()).unwrap();
        original.await.unwrap();
        assert!(destroyed.load(Ordering::Acquire));
        if error == InputError::Cleanup {
            assert!(cleanup.wait().await.is_err());
            assert_eq!(completion.wait().await, Err(InputError::Cleanup));
            assert_eq!(completion.cleanup(), CleanupState::Failed);
        } else {
            cleanup.wait().await.unwrap();
            assert_eq!(completion.wait().await, Ok(CleanupState::Restored));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_same_peer_subscription_cannot_erase_the_retired_original_inside_frame_age() {
    use crate::media::MediaEngine;
    use crate::video::{VideoDelivery, VideoReceiver};
    use libwebrtc::data_channel::DataChannelInit;
    use libwebrtc::peer_connection_factory::{
        native::PeerConnectionFactoryExt, PeerConnectionFactory,
    };
    use libwebrtc::video_frame::{I420Buffer, VideoFrame, VideoRotation};
    use libwebrtc::video_source::{native::NativeVideoSource, VideoResolution};
    let cleanup = Arc::new(AudioCleanup::default());
    let (mut engine, _events) = MediaEngine::new_data_only("local".into());
    engine.authorize_peers(["remote".into()]);
    engine
        .create_data_channel(
            "remote",
            "rc-frame-source-fixture",
            DataChannelInit::default(),
        )
        .unwrap();
    let lifetime = engine.peer_lifetime("remote").unwrap();
    let factory = PeerConnectionFactory::default();
    let source = NativeVideoSource::new(
        VideoResolution {
            width: 160,
            height: 96,
        },
        true,
    );
    let track = factory.create_video_track("rc-frame-source", source.clone());
    let (sender, mut frames) = tokio::sync::mpsc::unbounded_channel::<VideoDelivery>();
    let mut receiver = VideoReceiver::with_cleanup(
        track,
        lifetime.clone(),
        Arc::new(move |frame| {
            let _ = sender.send(frame);
        }),
        cleanup.clone(),
    );
    let next = || {
        source.capture_frame(&VideoFrame::new(
            VideoRotation::VideoRotation0,
            I420Buffer::new_black(160, 96),
        ));
    };
    receiver.start();
    let original = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            next();
            tokio::select! { frame = frames.recv() => break frame.unwrap(), _ = tokio::time::sleep(Duration::from_millis(5)) => {} }
        }
    }).await.unwrap();
    let stamp = original.frame.stamp(&original.lease);
    let selected = P2pFrameSource::from_delivery(stamp.clone(), None).unwrap();
    assert!(P2pFrameSource::from_delivery(stamp.clone(), Some(1)).is_none());
    let history = FrameHistory {
        source: Some(selected.clone()),
        latest: Some(stamp.clone()),
    };
    assert!(history.source_current(&selected.clone()));
    let other = P2pFrameSource::from_delivery(stamp.clone(), None).unwrap();
    assert!(!history.source_current(&other));
    receiver.stop();
    receiver.start();
    let fresh = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            next();
            tokio::select! { frame = frames.recv() => {
                let frame = frame.unwrap();
                if frame.lease.is_alive() { break frame; }
            }, _ = tokio::time::sleep(Duration::from_millis(5)) => {} }
        }
    })
    .await
    .unwrap();
    let fresh_stamp = fresh.frame.stamp(&fresh.lease);
    let replacement = P2pFrameSource::from_delivery(fresh_stamp.clone(), None).unwrap();
    assert!(lifetime.is_alive());
    assert!(!selected.original.is_alive());
    assert!(fresh_stamp.is_current_since(&lifetime, stamp.decoded_at()));
    assert!(
        fresh_stamp.decoded_at().duration_since(stamp.decoded_at()) < Duration::from_millis(750)
    );
    assert!(!history.source_current(&replacement));
    assert_eq!(history.latest.unwrap().decoded_at(), stamp.decoded_at());
    receiver.stop();
    drop(receiver);
    cleanup.wait().await.unwrap();
    engine.close();
}

struct FakePill {
    fail: Rc<Cell<bool>>,
    destroyed: Rc<Cell<bool>>,
    shown: Rc<RefCell<Vec<Rect>>>,
    observation: zabor_input::desktop::CleanupObservation,
}

impl PillBackend for FakePill {
    fn size(&self) -> Option<(i32, i32)> {
        Some((160, 32))
    }

    fn present(&mut self, bounds: Rect, authorized: &dyn Fn() -> bool) -> Result<(), InputError> {
        if !authorized() {
            return Err(InputError::Unavailable);
        }
        self.shown.borrow_mut().push(bounds);
        Ok(())
    }

    fn finish(&mut self) -> Result<(), InputError> {
        if self.fail.get() {
            return Err(InputError::Cleanup);
        }
        zabor_input::desktop::run_viewer_cursor(
            ViewerCursorLease::new(0),
            || true,
            self.observation.clone(),
        )
    }
}

impl Drop for FakePill {
    fn drop(&mut self) {
        self.destroyed.set(true);
    }
}

fn capture() -> Arc<CaptureLease> {
    let capture = CaptureLease::new(None);
    capture.update(Rect {
        left: -1920,
        top: -120,
        width: 1920,
        height: 1080,
    });
    capture
}

type PillFixture = (
    PillSurface,
    CursorCompletion,
    Rc<Cell<bool>>,
    Rc<RefCell<Vec<Rect>>>,
);

fn synthetic_pill(
    cleanup: Arc<AudioCleanup>,
    permitted: Rc<Cell<bool>>,
    fail: Rc<Cell<bool>>,
) -> PillFixture {
    let (done, completion) = register_surface(cleanup).unwrap();
    let destroyed = Rc::new(Cell::new(false));
    let shown = Rc::new(RefCell::new(Vec::new()));
    let state = PillSurface {
        backend: Some(Box::new(FakePill {
            fail,
            destroyed: destroyed.clone(),
            shown: shown.clone(),
            observation: completion.observation(),
        })),
        capture: capture(),
        authorized: Box::new(move || permitted.get()),
        done: Some(done),
        retired: false,
        failed: false,
    };
    (state, completion, destroyed, shown)
}

#[tokio::test(flavor = "current_thread")]
async fn original_ui_surface_is_destroyed_before_registered_completion_and_cannot_reshow() {
    let cleanup = Arc::new(AudioCleanup::default());
    let permitted = Rc::new(Cell::new(true));
    let (mut pill, mut completion, destroyed, shown) = synthetic_pill(
        cleanup.clone(),
        permitted.clone(),
        Rc::new(Cell::new(false)),
    );
    pill.update().unwrap();
    assert_eq!(
        shown.borrow()[0],
        Rect {
            left: -1040,
            top: -110,
            width: 160,
            height: 32
        }
    );
    assert!(!destroyed.get());
    assert!(
        tokio::time::timeout(Duration::from_millis(20), cleanup.wait())
            .await
            .is_err()
    );
    permitted.set(false);
    assert_eq!(pill.update(), Err(InputError::Unavailable));
    assert!(destroyed.get());
    cleanup.wait().await.unwrap();
    assert_eq!(completion.wait().await, Ok(CleanupState::Restored));
    permitted.set(true);
    assert_eq!(pill.update(), Err(InputError::Unavailable));
    assert_eq!(shown.borrow().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn ui_cleanup_failure_remains_registered_and_sticky_after_successful_retry() {
    let cleanup = Arc::new(AudioCleanup::default());
    let fail = Rc::new(Cell::new(true));
    let (mut pill, mut completion, destroyed, _) =
        synthetic_pill(cleanup.clone(), Rc::new(Cell::new(true)), fail.clone());
    pill.update().unwrap();
    assert_eq!(pill.finish(), Err(InputError::Cleanup));
    assert!(!destroyed.get());
    assert_eq!(completion.state(), CursorState::Starting);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), cleanup.wait())
            .await
            .is_err()
    );
    fail.set(false);
    assert_eq!(pill.finish(), Err(InputError::Cleanup));
    assert!(destroyed.get());
    assert!(cleanup.wait().await.is_err());
    assert_eq!(completion.wait().await, Err(InputError::Cleanup));
    assert_eq!(completion.cleanup(), CleanupState::Failed);
    assert_eq!(pill.finish(), Err(InputError::Cleanup));
}

#[tokio::test(flavor = "current_thread")]
async fn capture_retirement_and_original_ui_drop_finish_only_their_registered_surface() {
    for revoked in [false, true] {
        let cleanup = Arc::new(AudioCleanup::default());
        let (mut pill, mut completion, destroyed, shown) = synthetic_pill(
            cleanup.clone(),
            Rc::new(Cell::new(true)),
            Rc::new(Cell::new(false)),
        );
        pill.update().unwrap();
        if revoked {
            pill.capture.revoke();
            assert_eq!(pill.update(), Err(InputError::Unavailable));
        }
        drop(pill);
        assert!(destroyed.get());
        assert_eq!(shown.borrow().len(), 1);
        cleanup.wait().await.unwrap();
        assert_eq!(completion.wait().await, Ok(CleanupState::Restored));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn lost_ui_sender_or_unproved_receipt_never_reports_cleanup_success() {
    for receipt in [None, Some(Ok(()))] {
        let cleanup = Arc::new(AudioCleanup::default());
        let (sender, mut completion) = register_surface(cleanup.clone()).unwrap();
        if let Some(receipt) = receipt {
            sender.send(receipt).unwrap();
        }
        drop(sender);
        assert!(cleanup.wait().await.is_err());
        assert_eq!(completion.wait().await, Err(InputError::Cleanup));
    }
}

#[test]
fn app_facing_desktop_records_are_thread_affine_while_receipts_are_shareable() {
    trait AmbiguousIfSend<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    struct SendMarker;
    impl<T: ?Sized + Send> AmbiguousIfSend<SendMarker> for T {}
    trait AmbiguousIfSync<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSync<()> for T {}
    struct SyncMarker;
    impl<T: ?Sized + Sync> AmbiguousIfSync<SyncMarker> for T {}
    let _ = <ViewerDesktop as AmbiguousIfSend<_>>::check;
    let _ = <ViewerDesktop as AmbiguousIfSync<_>>::check;
    let _ = <HostPill as AmbiguousIfSend<_>>::check;
    let _ = <HostPill as AmbiguousIfSync<_>>::check;
    fn shareable<T: Send + Sync>() {}
    shareable::<CursorCompletion>();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adversarial_rc_v5_provenance_fence_sfu_and_monotonicity_stress_test() {
    // 1. FRAME PROVENANCE TIMEOUT:
    // Unconfirmed frame provenance drops lease and drops input dispatch.
    {
        let lease = ViewerCursorLease::new(42);
        assert!(lease.is_alive());
        let t0 = Instant::now();
        lease.acknowledge_at(t0);
        let viewport = Rect {
            left: 0,
            top: 0,
            width: 1920,
            height: 1080,
        };
        lease.decoded_p2p_frame(viewport, (1920, 1080), t0);
        assert!(lease.is_alive());
        assert!(lease.is_ready());

        // Frame admitted at exact edge (750ms)
        assert!(frame_admitted(
            None,
            true,
            t0,
            t0,
            t0 + Duration::from_millis(750)
        ));
        // Past edge (751ms) -> timeout drops admission
        assert!(!frame_admitted(
            None,
            true,
            t0,
            t0,
            t0 + Duration::from_millis(751)
        ));
        // Stale frame before ack -> unconfirmed provenance drops admission
        assert!(!frame_admitted(
            None,
            true,
            t0 - Duration::from_millis(1),
            t0,
            t0
        ));
        // Premature frame in the future -> drops admission
        assert!(!frame_admitted(
            None,
            true,
            t0 + Duration::from_millis(50),
            t0,
            t0
        ));

        // Lease revocation on frame clear drops input readiness and liveliness
        lease.clear_frame();
        assert!(!lease.is_alive());
        assert!(!lease.is_ready());
    }

    // 2. REJECTION OF SFU FRAMES AS DIRECT P2P SOURCES:
    // Frames with source_epoch != None are SFU routed and must be rejected.
    {
        let ack = Instant::now();
        let decoded = ack;
        let now = ack + Duration::from_millis(100);

        // Direct P2P (source_epoch is None) is admitted
        assert!(frame_admitted(None, true, decoded, ack, now));

        // Any SFU route epoch is strictly rejected
        for sfu_epoch in [0, 1, 2, 7, 42, 1024, u64::MAX] {
            assert!(
                !frame_admitted(Some(sfu_epoch), true, decoded, ack, now),
                "SFU epoch {sfu_epoch} must never be admitted as direct P2P source"
            );
        }
    }

    // 3. FENCE DISARM:
    // When host exits, peer connection drops, or authorization is lost,
    // control fences disarm and cursor capture releases.
    {
        let cleanup = Arc::new(AudioCleanup::default());
        let permitted = Rc::new(Cell::new(true));
        let fail = Rc::new(Cell::new(false));
        let (mut pill, mut completion, destroyed, shown) =
            synthetic_pill(cleanup.clone(), permitted.clone(), fail.clone());
        pill.update().unwrap();
        assert_eq!(shown.borrow().len(), 1);
        assert!(!destroyed.get());

        // Host fence disarm: permitted becomes false (host exits / connection drops)
        permitted.set(false);
        assert_eq!(pill.update(), Err(InputError::Unavailable));
        assert!(destroyed.get());

        // Cleanup completes successfully, restoring cursor capture bounds
        cleanup.wait().await.unwrap();
        assert_eq!(completion.wait().await, Ok(CleanupState::Restored));

        // Re-enabling permission afterwards CANNOT revive the destroyed/retired pill
        permitted.set(true);
        assert_eq!(pill.update(), Err(InputError::Unavailable));
        assert_eq!(shown.borrow().len(), 1);
    }

    // 4. MONOTONIC INTENT SEQUENCE NUMBERS & REPLAY REJECTION:
    // Reject replayed or reordered input packets and stale tickets.
    {
        let intents = Arc::new(CriticalIntents::default());
        let t1 = intents.ticket();
        assert!(intents.is_current(t1, IntentScope::Session));
        assert!(intents.is_current(t1, IntentScope::Voice));

        // Voice intent changes on Leave
        intents.request(CriticalAction::Leave);
        assert!(!intents.is_current(t1, IntentScope::Voice));
        assert!(intents.is_current(t1, IntentScope::Session));

        // Monotonic step to ticket T2
        let t2 = intents.take_leave(t1).expect("take_leave succeeds");
        assert_ne!(t1, t2);
        assert!(intents.is_current(t2, IntentScope::Voice));
        assert!(
            !intents.is_current(t1, IntentScope::Voice),
            "Replayed T1 ticket must be rejected"
        );

        // Logout invalidates both T1 and T2
        intents.request(CriticalAction::Logout);
        assert!(!intents.is_current(t1, IntentScope::Session));
        assert!(!intents.is_current(t1, IntentScope::Voice));
        assert!(!intents.is_current(t2, IntentScope::Session));
        assert!(!intents.is_current(t2, IntentScope::Voice));

        // ViewerRequest sequence / identity replay rejection
        let r1 = ViewerRequest::default();
        let r2 = ViewerRequest::default();
        let now = Instant::now();
        assert!(r1.accepts_ack(&r1, now));
        assert!(!r2.accepts_ack(&r1, now));
        assert!(!r1.accepts_ack(&r2, now));
    }
}
