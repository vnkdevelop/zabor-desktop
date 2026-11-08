use super::*;

fn desktop() -> Rect {
    Rect {
        left: -1920,
        top: -120,
        width: 3840,
        height: 2280,
    }
}

fn viewport() -> Rect {
    Rect {
        left: 10,
        top: 20,
        width: 800,
        height: 600,
    }
}

struct FakeCursor {
    clipping: Rect,
    foreground: bool,
    fail_restore: bool,
    fail_finish: bool,
    fail_acquire: bool,
    forbid_clip: bool,
    poisoned: bool,
    calls: Vec<Option<Rect>>,
    finished: bool,
    held_restore: Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>,
}

impl Default for FakeCursor {
    fn default() -> Self {
        Self {
            clipping: desktop(),
            foreground: true,
            fail_restore: false,
            fail_finish: false,
            fail_acquire: false,
            forbid_clip: false,
            poisoned: false,
            calls: Vec::new(),
            finished: false,
            held_restore: None,
        }
    }
}

impl CursorBackend for FakeCursor {
    fn foreground(&self) -> bool {
        self.foreground
    }
    fn screen_rect(&self, client: Rect) -> Result<Rect, InputError> {
        let bounds = Rect {
            left: 0,
            top: 0,
            width: 1000,
            height: 800,
        };
        if !contains_rect(bounds, client) {
            return Err(InputError::Unavailable);
        }
        Ok(Rect {
            left: client.left - 1800,
            top: client.top - 100,
            ..client
        })
    }
    fn clipping(&self) -> Result<Rect, InputError> {
        if self.fail_acquire {
            Err(InputError::Unavailable)
        } else {
            Ok(self.clipping)
        }
    }
    fn desktop(&self) -> Result<Rect, InputError> {
        Ok(desktop())
    }
    fn clip(&mut self, rect: Option<Rect>) -> Result<(), InputError> {
        assert!(
            !self.forbid_clip,
            "cancelled synthetic owner attempted clipping"
        );
        self.calls.push(rect);
        if rect.is_none() {
            if let Some((entered, release)) = self.held_restore.take() {
                entered.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(3)).unwrap();
            }
        }
        if self.fail_restore && rect.is_none() {
            return Err(InputError::Unavailable);
        }
        self.clipping = rect.unwrap_or_else(desktop);
        Ok(())
    }
    fn poison(&mut self) {
        self.poisoned = true;
    }
    fn finish(&mut self) -> Result<(), InputError> {
        if self.fail_finish {
            Err(InputError::Cleanup)
        } else {
            self.finished = true;
            Ok(())
        }
    }
}

fn owner(backend: FakeCursor) -> CursorOwner<FakeCursor> {
    CursorOwner::acquire(backend, CleanupObservation::default()).unwrap()
}

fn ready_lease() -> Arc<ViewerCursorLease> {
    let lease = ViewerCursorLease::new(123);
    lease.acknowledge();
    lease.decoded_p2p_frame(viewport(), (1920, 1080), Instant::now());
    lease
}

#[test]
fn original_ack_handoff_preserves_decoder_age_and_cannot_replace_request_ack() {
    let accepted = Instant::now() - Duration::from_millis(50);
    let decoded = accepted + Duration::from_millis(10);
    let lease = ViewerCursorLease::new(123);
    lease.acknowledge_at(accepted);
    lease.decoded_p2p_frame(viewport(), (1920, 1080), decoded);
    assert!(lease.is_ready());
    lease.acknowledge_at(accepted);
    assert_eq!(lease.state.lock().unwrap().frame.unwrap().2, decoded);
    lease.set_confined(false);
    assert!(lease.is_alive());
    assert!(!lease.is_ready());
    lease.acknowledge_at(accepted + Duration::from_nanos(1));
    assert!(!lease.is_alive());
    let future = ViewerCursorLease::new(123);
    future.acknowledge_at(Instant::now() + Duration::from_secs(1));
    assert!(!future.is_alive());
}

#[test]
fn contained_video_geometry_preserves_negative_origins_and_excludes_letterboxing() {
    let rect = fitted_video_rect(viewport(), (1920, 1080)).unwrap();
    assert_eq!(
        rect,
        Rect {
            left: 10,
            top: 95,
            width: 800,
            height: 450
        }
    );
    assert_eq!(
        fitted_video_rect(
            Rect {
                left: -100,
                top: -50,
                width: 801,
                height: 600
            },
            (16, 9)
        ),
        Some(Rect {
            left: -100,
            top: 25,
            width: 801,
            height: 450
        })
    );
    assert_eq!(
        fitted_video_rect(viewport(), (600, 800)),
        Some(Rect {
            left: 185,
            top: 20,
            width: 450,
            height: 600
        })
    );
    assert_eq!(
        fitted_video_rect(
            Rect {
                width: 1,
                height: 1,
                ..viewport()
            },
            (1, 1)
        ),
        Some(Rect {
            width: 1,
            height: 1,
            ..viewport()
        })
    );
    for rect in [
        Rect {
            width: 0,
            ..viewport()
        },
        Rect {
            left: i32::MAX,
            ..viewport()
        },
    ] {
        assert!(fitted_video_rect(rect, (1920, 1080)).is_none());
    }
    for pixels in [(0, 1), (1, 0), (u32::MAX, 1)] {
        assert!(fitted_video_rect(viewport(), pixels).is_none());
    }
}

#[test]
fn host_pill_placement_uses_original_negative_capture_bounds_not_primary_display() {
    let capture = Rect {
        left: -1920,
        top: -120,
        width: 1920,
        height: 1080,
    };
    assert_eq!(
        host_pill_bounds(capture, (160, 32)),
        Some(Rect {
            left: -1040,
            top: -110,
            width: 160,
            height: 32
        })
    );
    assert!(host_pill_bounds(
        Rect {
            width: 159,
            ..capture
        },
        (160, 32)
    )
    .is_none());
    assert!(host_pill_bounds(
        Rect {
            height: 41,
            ..capture
        },
        (160, 32)
    )
    .is_none());
    assert!(host_pill_bounds(capture, (0, 32)).is_none());
}

#[test]
fn viewport_conversion_uses_current_dpi_and_rounds_inward_without_monitor_fallback() {
    for (scale, left, top, width, height) in [
        (1.0, 10, 20, 800, 600),
        (1.25, 13, 25, 999, 750),
        (1.5, 15, 30, 1200, 900),
        (2.0, 20, 40, 1600, 1200),
    ] {
        assert_eq!(
            physical_viewport([10.0, 20.0, 800.0, 600.0], scale),
            Some(Rect {
                left,
                top,
                width,
                height
            })
        );
    }
    assert_eq!(
        physical_viewport([-10.25, -20.25, 800.0, 600.0], 1.5),
        Some(Rect {
            left: -15,
            top: -30,
            width: 1199,
            height: 899
        })
    );
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(physical_viewport([0.0, 0.0, 1.0, 1.0], scale).is_none());
    }
    for bounds in [
        [0.0, 0.0, 0.0, 1.0],
        [f64::NAN, 0.0, 1.0, 1.0],
        [f64::from(i32::MAX), 0.0, 1.0, 1.0],
        [0.0, 0.0, 0.25, 0.25],
    ] {
        assert!(physical_viewport(bounds, 1.0).is_none());
    }
}

#[test]
fn acknowledgement_requires_a_new_decoded_frame_and_repeated_ack_does_not_restart_it() {
    let lease = ViewerCursorLease::new(123);
    let before = Instant::now();
    lease.decoded_p2p_frame(viewport(), (1920, 1080), before);
    assert_eq!(lease.snapshot(Instant::now()), Some(None));
    lease.acknowledge();
    lease.decoded_p2p_frame(viewport(), (1920, 1080), before);
    assert_eq!(lease.snapshot(Instant::now()), Some(None));
    lease.decoded_p2p_frame(viewport(), (1920, 1080), Instant::now());
    let ready = lease.snapshot(Instant::now());
    assert!(matches!(ready, Some(Some(_))));
    lease.acknowledge();
    assert_eq!(lease.snapshot(Instant::now()), ready);
}

#[test]
fn frame_freshness_is_not_renewed_by_resize_cached_callbacks_or_ack() {
    let lease = ready_lease();
    let decoded_at = lease.state.lock().unwrap().frame.unwrap().2;
    lease.relayout(Rect {
        width: 600,
        ..viewport()
    });
    assert!(matches!(
        lease.snapshot(decoded_at + FRAME_AGE),
        Some(Some(_))
    ));
    lease.acknowledge();
    lease.activity();
    assert_eq!(
        lease.snapshot(decoded_at + FRAME_AGE + Duration::from_nanos(1)),
        None
    );
    lease.decoded_p2p_frame(
        viewport(),
        (1920, 1080),
        decoded_at - Duration::from_secs(1),
    );
    assert_eq!(lease.state.lock().unwrap().frame.unwrap().2, decoded_at);
}

#[test]
fn retirement_and_fallback_cannot_be_revived_by_late_original_callbacks() {
    for lease in [ready_lease(), ready_lease()] {
        lease.clear_frame();
        lease.acknowledge();
        lease.set_confined(true);
        lease.decoded_p2p_frame(viewport(), (1920, 1080), Instant::now());
        lease.relayout(viewport());
        lease.activity();
        assert_eq!(lease.snapshot(Instant::now()), None);
    }
    assert_eq!(ViewerCursorLease::new(0).snapshot(Instant::now()), None);
}

#[test]
fn observed_stall_is_terminal_even_when_a_fresh_original_frame_arrives_later() {
    let lease = ready_lease();
    let at = lease.state.lock().unwrap().frame.unwrap().2;
    assert_eq!(
        lease.snapshot(at + FRAME_AGE + Duration::from_nanos(1)),
        None
    );
    lease.acknowledge();
    lease.activity();
    lease.set_confined(true);
    lease.decoded_p2p_frame(viewport(), (1920, 1080), Instant::now());
    assert!(!lease.is_alive());
}

#[test]
fn fresh_callbacks_cannot_hide_an_unobserved_original_frame_stall() {
    let lease = ready_lease();
    lease.state.lock().unwrap().frame.as_mut().unwrap().2 =
        Instant::now() - FRAME_AGE - Duration::from_millis(1);
    lease.decoded_p2p_frame(viewport(), (1920, 1080), Instant::now());
    lease.activity();
    assert!(!lease.is_alive());
}

#[test]
fn permission_and_frame_inactivity_retire_without_native_calls() {
    let pending = ViewerCursorLease::new(123);
    assert_eq!(pending.snapshot(pending.requested + INACTIVITY), None);
    let ready = ready_lease();
    let at = ready.state.lock().unwrap().activity.unwrap();
    assert_eq!(ready.snapshot(at + INACTIVITY), None);
    ready.decoded_p2p_frame(
        Rect {
            width: 0,
            ..viewport()
        },
        (1920, 1080),
        Instant::now(),
    );
    assert_eq!(ready.snapshot(Instant::now()), None);
}

#[test]
fn confinement_maps_actual_client_origin_and_restores_once_without_warping() {
    let mut owner = owner(FakeCursor::default());
    let observation = owner.observation.clone();
    let client = fitted_video_rect(viewport(), (1920, 1080)).unwrap();
    assert!(owner.apply(Some(client)).unwrap());
    assert_eq!(
        owner.backend.clipping,
        Rect {
            left: -1790,
            top: -5,
            width: 800,
            height: 450
        }
    );
    assert!(owner.apply(Some(client)).unwrap());
    assert_eq!(owner.backend.calls.len(), 1);
    owner.finish().unwrap();
    owner.finish().unwrap();
    assert_eq!(
        owner.backend.calls,
        vec![Some(owner.backend.screen_rect(client).unwrap()), None]
    );
    assert!(owner.backend.finished);
    assert_eq!(observation.state(), CleanupState::Restored);
}

#[test]
fn existing_restrictions_are_preserved_and_never_broadened() {
    let original = Rect {
        left: -1800,
        top: -100,
        width: 1000,
        height: 800,
    };
    let mut owner = owner(FakeCursor {
        clipping: original,
        ..Default::default()
    });
    owner.apply(Some(viewport())).unwrap();
    owner.apply(None).unwrap();
    assert_eq!(owner.backend.clipping, original);
    owner.apply(Some(viewport())).unwrap();
    owner.finish().unwrap();
    assert_eq!(owner.backend.clipping, original);
    let mut constrained = owner_from_restriction();
    assert_eq!(
        constrained.apply(Some(viewport())),
        Err(InputError::Unavailable)
    );
    assert!(constrained.backend.calls.is_empty());
}

fn owner_from_restriction() -> CursorOwner<FakeCursor> {
    owner(FakeCursor {
        clipping: Rect {
            left: -1800,
            top: -100,
            width: 100,
            height: 100,
        },
        ..Default::default()
    })
}

#[test]
fn another_cursor_owner_is_never_overwritten_or_unclipped() {
    for acquired in [false, true] {
        let mut owner = owner(FakeCursor::default());
        if acquired {
            owner.apply(Some(viewport())).unwrap();
        }
        let external = Rect {
            left: 0,
            top: 0,
            width: 100,
            height: 100,
        };
        owner.backend.clipping = external;
        let calls = owner.backend.calls.len();
        assert!(!owner.apply(Some(viewport())).unwrap());
        owner.finish().unwrap();
        assert_eq!(owner.backend.clipping, external);
        assert_eq!(owner.backend.calls.len(), calls);
        assert_eq!(owner.observation.state(), CleanupState::Superseded);
    }
}

#[test]
fn foreground_loss_never_blocks_restoration_and_bad_client_geometry_is_rejected() {
    let mut owner = owner(FakeCursor::default());
    assert_eq!(
        owner.apply(Some(Rect {
            left: -1,
            ..viewport()
        })),
        Err(InputError::Unavailable)
    );
    owner.apply(Some(viewport())).unwrap();
    owner.backend.foreground = false;
    assert!(!owner.apply(Some(viewport())).unwrap());
    owner.finish().unwrap();
    assert_eq!(owner.backend.clipping, desktop());
}

#[test]
fn failed_restore_or_backend_retirement_is_observable_and_sticky_after_retry() {
    for fail_finish in [false, true] {
        let mut owner = owner(FakeCursor::default());
        let observation = owner.observation.clone();
        owner.apply(Some(viewport())).unwrap();
        owner.backend.fail_restore = !fail_finish;
        owner.backend.fail_finish = fail_finish;
        assert_eq!(owner.finish(), Err(InputError::Cleanup));
        assert_eq!(observation.state(), CleanupState::Failed);
        assert!(owner.backend.poisoned);
        owner.backend.fail_restore = false;
        owner.backend.fail_finish = false;
        owner.finish().unwrap();
        assert_eq!(observation.state(), CleanupState::Failed);
    }
}

#[test]
fn cancelled_original_actor_never_clips_and_retired_actor_ignores_late_frames() {
    let lease = ready_lease();
    let observation = CleanupObservation::default();
    drive_cursor(
        FakeCursor {
            forbid_clip: true,
            ..Default::default()
        },
        &lease,
        || true,
        observation.clone(),
        || panic!("cancelled actor waited"),
    )
    .unwrap();
    assert_eq!(observation.state(), CleanupState::Restored);
    assert_eq!(lease.snapshot(Instant::now()), None);
    let lease = ready_lease();
    let calls = std::cell::Cell::new(0);
    drive_cursor(
        FakeCursor {
            forbid_clip: true,
            ..Default::default()
        },
        &lease,
        || {
            let at = calls.get();
            calls.set(at + 1);
            at != 0
        },
        CleanupObservation::default(),
        || panic!("cancelled admission waited"),
    )
    .unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(lease.snapshot(Instant::now()), None);
    let lease = ready_lease();
    let observation = CleanupObservation::default();
    drive_cursor(
        FakeCursor::default(),
        &lease,
        || false,
        observation.clone(),
        || lease.revoke(),
    )
    .unwrap();
    lease.acknowledge();
    lease.decoded_p2p_frame(viewport(), (1920, 1080), Instant::now());
    assert_eq!(lease.snapshot(Instant::now()), None);
    assert_eq!(observation.state(), CleanupState::Restored);
}

#[test]
fn request_retirement_precedes_the_original_blocked_cursor_restore() {
    use std::sync::atomic::AtomicUsize;
    let lease = ready_lease();
    let observation = CleanupObservation::default();
    let (entered, restoring) = std::sync::mpsc::channel();
    let (release, held) = std::sync::mpsc::channel();
    let stopped = Arc::new(AtomicUsize::new(0));
    let worker_stopped = stopped.clone();
    let worker_observation = observation.clone();
    let thread = std::thread::spawn(move || {
        drive_cursor_with_retirement(
            FakeCursor {
                held_restore: Some((entered, held)),
                ..Default::default()
            },
            &lease,
            || false,
            worker_observation,
            || lease.revoke(),
            &|| {
                worker_stopped.store(1, Ordering::Release);
            },
        )
    });
    restoring.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(stopped.load(Ordering::Acquire), 1);
    assert_eq!(observation.state(), CleanupState::Pending);
    release.send(()).unwrap();
    thread.join().unwrap().unwrap();
    assert_eq!(observation.state(), CleanupState::Restored);
}

#[cfg(windows)]
#[test]
fn framework_style_and_show_mutations_are_filtered_before_visibility_not_after_it() {
    use windows::Win32::UI::WindowsAndMessaging::*;
    let native_passive = passive_host_style(WS_EX_APPWINDOW.0 | WS_EX_LAYERED.0);
    assert_eq!(native_passive & WS_EX_APPWINDOW.0, 0);
    for framework_style in [
        WS_EX_APPWINDOW.0,
        WS_EX_APPWINDOW.0 | WS_EX_TOPMOST.0,
        WS_EX_ACCEPTFILES.0,
    ] {
        let style_before_show = passive_host_style(framework_style);
        assert_eq!(style_before_show, passive_host_style(style_before_show));
        assert_eq!(
            style_before_show & (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT).0,
            (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT).0
        );
        assert_eq!(style_before_show & WS_EX_APPWINDOW.0, 0);
        let shown = passive_position_flags(SWP_SHOWWINDOW | SWP_FRAMECHANGED);
        assert!(shown.contains(SWP_NOACTIVATE | SWP_SHOWWINDOW));
        assert_eq!(
            style_before_show & WS_EX_TOPMOST.0,
            framework_style & WS_EX_TOPMOST.0
        );
    }
}

#[test]
fn drop_runs_cleanup_without_claiming_success_before_it_and_poison_survives() {
    let observation = CleanupObservation::default();
    {
        let mut owner = CursorOwner::acquire(FakeCursor::default(), observation.clone()).unwrap();
        owner.apply(Some(viewport())).unwrap();
        assert_eq!(observation.state(), CleanupState::Pending);
        owner.backend.fail_restore = true;
    }
    assert_eq!(observation.state(), CleanupState::Failed);
    let result = CursorOwner::acquire(
        FakeCursor {
            fail_acquire: true,
            ..Default::default()
        },
        CleanupObservation::default(),
    );
    assert!(matches!(result, Err(InputError::Unavailable)));
}

#[test]
fn unwind_retires_original_lease_and_observes_synthetic_cleanup() {
    let lease = ready_lease();
    let observation = CleanupObservation::default();
    let result = std::panic::catch_unwind(|| {
        drive_cursor(
            FakeCursor::default(),
            &lease,
            || false,
            observation.clone(),
            || panic!("synthetic owner unwind"),
        )
    });
    assert!(result.is_err());
    assert_eq!(lease.snapshot(Instant::now()), None);
    assert_eq!(observation.state(), CleanupState::Restored);
}

#[test]
fn desktop_owners_are_thread_confined_and_lease_observers_are_shareable() {
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
    let _ = <CursorOwner<FakeCursor> as AmbiguousIfSend<_>>::check;
    let _ = <CursorOwner<FakeCursor> as AmbiguousIfSync<_>>::check;
    let _ = <SurfaceOwner<FakeSurface> as AmbiguousIfSend<_>>::check;
    let _ = <SurfaceOwner<FakeSurface> as AmbiguousIfSync<_>>::check;
    #[cfg(windows)]
    {
        let _ = <HostSurface as AmbiguousIfSend<_>>::check;
        let _ = <HostSurface as AmbiguousIfSync<_>>::check;
        let _ = <PassiveHostWindow as AmbiguousIfSend<_>>::check;
        let _ = <PassiveHostWindow as AmbiguousIfSync<_>>::check;
    }
    fn shareable<T: Send + Sync>() {}
    shareable::<ViewerCursorLease>();
    shareable::<CleanupObservation>();
}

#[derive(Default)]
struct FakeSurface {
    current: bool,
    shown: Vec<Rect>,
    restored: usize,
    fail_show: bool,
    fail_restore: bool,
}

impl SurfaceBackend for FakeSurface {
    fn current(&self) -> bool {
        self.current
    }
    fn present(&mut self, bounds: Rect) -> Result<(), InputError> {
        self.shown.push(bounds);
        if self.fail_show {
            Err(InputError::Unavailable)
        } else {
            Ok(())
        }
    }
    fn restore(&mut self) -> Result<(), InputError> {
        self.restored += 1;
        if self.fail_restore {
            Err(InputError::Cleanup)
        } else {
            Ok(())
        }
    }
}

fn surface() -> SurfaceOwner<FakeSurface> {
    SurfaceOwner {
        backend: FakeSurface {
            current: true,
            ..Default::default()
        },
        observation: CleanupObservation::default(),
        finished: false,
        affinity: PhantomData,
    }
}

#[test]
fn host_surface_revocation_is_terminal_and_late_updates_cannot_reshow_it() {
    let mut surface = surface();
    surface.present(viewport(), true).unwrap();
    assert_eq!(
        surface.present(viewport(), false),
        Err(InputError::Unavailable)
    );
    assert_eq!(
        surface.present(viewport(), true),
        Err(InputError::Unavailable)
    );
    surface.finish().unwrap();
    assert_eq!(surface.backend.shown, vec![viewport()]);
    assert_eq!(surface.backend.restored, 1);
    assert_eq!(surface.observation.state(), CleanupState::Restored);
}

#[test]
fn host_surface_stale_window_invalid_geometry_or_failed_show_cleanup_remains_observed() {
    for reason in 0..3 {
        let mut surface = surface();
        surface.backend.current = reason != 0;
        surface.backend.fail_show = reason == 2;
        let bounds = if reason == 1 {
            Rect {
                width: 0,
                ..viewport()
            }
        } else {
            viewport()
        };
        assert_eq!(surface.present(bounds, true), Err(InputError::Unavailable));
        assert_eq!(surface.backend.restored, 1);
        assert_eq!(surface.observation.state(), CleanupState::Restored);
    }
    let mut surface = surface();
    surface.backend.fail_restore = true;
    assert_eq!(surface.finish(), Err(InputError::Cleanup));
    surface.backend.fail_restore = false;
    surface.finish().unwrap();
    assert_eq!(surface.observation.state(), CleanupState::Failed);
}
