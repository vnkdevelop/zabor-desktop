use super::*;

#[test]
fn source_resolution_excludes_current_tree_and_includes_exact_window_tree_without_fallback() {
    let screen = resolve(Source::Screen, 17, |_| {
        panic!("Screen must not resolve a window")
    })
    .unwrap();
    assert_eq!(
        screen,
        Target {
            process_id: 17,
            include_tree: false
        }
    );
    let window = resolve(Source::Window(98765), 17, |handle| {
        assert_eq!(handle, 98765);
        Ok(41)
    })
    .unwrap();
    assert_eq!(
        window,
        Target {
            process_id: 41,
            include_tree: true
        }
    );
    assert!(resolve(Source::Window(0), 17, |_| panic!("Invalid HWND")).is_err());
    assert!(resolve(Source::Window(99), 17, |_| Ok(0)).is_err());
    assert!(resolve(Source::Window(99), 17, |_| Err(CaptureError::Unavailable)).is_err());
    assert!(resolve(Source::Screen, 0, |_| Ok(41)).is_err());
}

#[test]
fn float_stereo_copy_preserves_channel_order_and_silent_null_buffer_needs_no_bytes() {
    let bytes: Vec<u8> = [-0.25_f32, 0.5, -0.75, 1.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    assert_eq!(decode(&bytes, 2, false).unwrap(), [-0.25, 0.5, -0.75, 1.0]);
    assert_eq!(decode(&[], 2, true).unwrap(), [0.0; 4]);
    assert!(decode(&bytes[..12], 2, false).is_err());
    assert!(decode(&[], MAX_PACKET_FRAMES + 1, true).is_err());
    assert!(decode(&[], MAX_PACKET_FRAMES + 1, false).is_err());
    assert!(decode(&[], 0, false).unwrap().is_empty());
}

#[test]
fn hostile_float_samples_cannot_propagate_nan_infinity_or_out_of_range_audio() {
    let bytes: Vec<u8> = [f32::NAN, f32::INFINITY, -2.0, 2.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    assert_eq!(decode(&bytes, 2, false).unwrap(), [0.0, 0.0, -1.0, 1.0]);
}

#[cfg(feature = "test-source")]
#[test]
fn cancelled_and_refused_readiness_never_publish_samples() {
    run_synthetic(
        Source::Screen,
        0,
        0,
        || true,
        |_| panic!("Cancelled acquisition"),
        |_| panic!("Cancelled delivery"),
    )
    .unwrap();
    run_synthetic(
        Source::Screen,
        0,
        0,
        || false,
        |target| {
            assert!(!target.include_tree());
            false
        },
        |_| panic!("Rejected readiness"),
    )
    .unwrap();
}

#[cfg(feature = "test-source")]
#[test]
fn read_and_cleanup_failures_are_distinct_and_original_owner_joins_after_delayed_cleanup() {
    let started = std::time::Instant::now();
    let thread = std::thread::spawn(|| {
        run_synthetic(
            Source::Window(9),
            2,
            100,
            || false,
            |target| {
                assert_eq!(target.process_id(), 5353);
                assert!(target.include_tree());
                true
            },
            |packet| {
                assert_eq!(packet.samples.len(), 960);
                assert_eq!(packet.samples[..2], [-0.25, 0.5]);
                false
            },
        )
    });
    thread.join().unwrap().unwrap();
    assert!(started.elapsed() >= std::time::Duration::from_millis(100));
    assert_eq!(
        run_synthetic(Source::Screen, 3, 0, || false, |_| true, |_| true),
        Err(CaptureError::Unavailable)
    );
    assert_eq!(
        run_synthetic(Source::Screen, 4, 0, || false, |_| true, |_| true),
        Err(CaptureError::Unavailable)
    );
    assert_eq!(
        run_synthetic(Source::Screen, 5, 0, || false, |_| true, |_| false),
        Err(CaptureError::Cleanup)
    );
}

#[cfg(feature = "test-source")]
#[test]
fn callback_unwind_retires_native_owner_before_the_original_thread_can_complete() {
    let started = std::time::Instant::now();
    let thread = std::thread::spawn(|| {
        run_synthetic(
            Source::Screen,
            2,
            100,
            || false,
            |_| true,
            |_| panic!("Synthetic stream audio actor unwind"),
        )
    });
    assert!(thread.join().is_err());
    assert!(started.elapsed() >= std::time::Duration::from_millis(100));
}
