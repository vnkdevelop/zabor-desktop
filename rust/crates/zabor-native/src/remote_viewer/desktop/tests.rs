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
