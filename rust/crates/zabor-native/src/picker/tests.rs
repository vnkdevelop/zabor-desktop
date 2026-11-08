use super::*;
use std::sync::mpsc;
use tokio::time::timeout;

#[test]
fn thumbnail_copy_converts_bgra_ignores_padding_and_retains_owned_opaque_pixels() {
    let mut bytes = vec![99; 20 * 2];
    for row in bytes.chunks_mut(20) {
        for pixel in row[..16].chunks_mut(4) {
            pixel.copy_from_slice(&[19, 37, 71, 0]);
        }
    }
    let image = PickerImage::from_bgra(&bytes, 20, 4, 2).unwrap();
    bytes.fill(0);
    assert_eq!((image.width, image.height), (4, 2));
    assert_eq!(image.rgba, [71, 37, 19, 255].repeat(8));
}

#[test]
fn thumbnails_are_bounded_aspect_preserving_and_reject_hostile_frame_layouts() {
    for (width, height, expected) in [
        (1920, 1080, (320, 180)),
        (1080, 1920, (101, 180)),
        (4096, 4096, (180, 180)),
        (1, 1, (1, 1)),
    ] {
        let bytes = [11, 22, 33, 255].repeat(width * height);
        let image =
            PickerImage::from_bgra(&bytes, width as u32 * 4, width as i32, height as i32).unwrap();
        assert_eq!((image.width, image.height), expected);
        assert!(image.rgba.len() <= 320 * 180 * 4);
        assert!(image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [33, 22, 11, 255]));
    }
    for (stride, width, height) in [
        (16, 0, 2),
        (16, 4, -1),
        (15, 4, 2),
        (16, 8193, 1),
        (16, 4, 8193),
        (u32::MAX, 4, 2),
    ] {
        assert!(PickerImage::from_bgra(&[0; 32], stride, width, height).is_err());
    }
    assert!(PickerImage::from_bgra(&[0; 31], 16, 4, 2).is_err());
}

#[test]
fn picker_cancellation_is_child_owned_and_does_not_revoke_the_voice_parent() {
    let parent = CancellationToken::new();
    let query = PickerQuery::new(parent.clone());
    query.cancel();
    assert!(query.is_cancelled());
    assert!(query.is_stopped());
    assert!(!parent.is_cancelled());
    let query = PickerQuery::new(parent.clone());
    parent.cancel();
    assert!(query.is_cancelled());
}

#[tokio::test]
async fn inventory_readiness_keeps_original_actor_registered_through_remaining_previews() {
    timeout(Duration::from_secs(5), async {
        let cleanup = Arc::new(AudioCleanup::default());
        let query = PickerQuery::new(CancellationToken::new());
        let (release, released) = mpsc::channel();
        assert_eq!(
            query
                .inventory(cleanup.clone(), move |_, ready| {
                    ready.send(7).unwrap();
                    released.recv().unwrap();
                    Ok(())
                })
                .await
                .unwrap(),
            7
        );
        assert!(query.is_running());
        assert!(!query.is_cancelled());
        cleanup.seal().unwrap();
        assert!(timeout(Duration::from_millis(40), cleanup.wait())
            .await
            .is_err());
        release.send(()).unwrap();
        cleanup.wait().await.unwrap();
        assert!(!query.is_running());
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn cancelled_or_aborted_inventory_wait_does_not_accept_native_completion_or_abort_owner() {
    timeout(Duration::from_secs(5), async {
        for abort in [false, true] {
            let cleanup = Arc::new(AudioCleanup::default());
            let query = PickerQuery::new(CancellationToken::new());
            let (entered, started) = oneshot::channel();
            let (release, released) = mpsc::channel();
            let owner_query = query.clone();
            let actor_cleanup = cleanup.clone();
            let wait = tokio::spawn(async move {
                owner_query
                    .inventory(actor_cleanup, move |query, ready| {
                        entered.send(()).unwrap();
                        released.recv().unwrap();
                        assert!(query.is_stopped());
                        assert!(ready.send(()).is_err());
                        Ok(())
                    })
                    .await
            });
            started.await.unwrap();
            if abort {
                wait.abort();
                assert!(wait.await.unwrap_err().is_cancelled());
                assert!(!query.is_cancelled());
            } else {
                query.cancel();
                assert!(wait.await.unwrap().is_err());
            }
            assert!(query.is_running());
            cleanup.seal().unwrap();
            assert!(timeout(Duration::from_millis(40), cleanup.wait())
                .await
                .is_err());
            release.send(()).unwrap();
            cleanup.wait().await.unwrap();
            assert!(!query.is_running());
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn cancelled_sealed_and_duplicate_queries_cannot_acquire_a_native_owner() {
    let cleanup = Arc::new(AudioCleanup::default());
    let query = PickerQuery::new(CancellationToken::new());
    query.cancel();
    assert!(query
        .inventory::<()>(cleanup.clone(), |_, _| panic!("cancelled acquisition"))
        .await
        .is_err());
    let query = PickerQuery::new(CancellationToken::new());
    cleanup.seal().unwrap();
    assert!(query
        .inventory::<()>(cleanup, |_, _| panic!("sealed acquisition"))
        .await
        .is_err());
    assert!(!query.is_running());
    let cleanup = Arc::new(AudioCleanup::default());
    let query = PickerQuery::new(CancellationToken::new());
    let (release, released) = mpsc::channel();
    query
        .inventory(cleanup.clone(), move |_, ready| {
            ready.send(()).unwrap();
            released.recv().unwrap();
            Ok(())
        })
        .await
        .unwrap();
    assert!(query
        .inventory::<()>(cleanup.clone(), |_, _| panic!("duplicate acquisition"))
        .await
        .is_err());
    release.send(()).unwrap();
    cleanup.wait().await.unwrap();
}

#[tokio::test]
async fn acquisition_failure_is_not_cleanup_success_and_actor_unwind_poisons_original_join() {
    let cleanup = Arc::new(AudioCleanup::default());
    let query = PickerQuery::new(CancellationToken::new());
    assert!(query
        .inventory::<()>(cleanup.clone(), |_, _| Ok(()))
        .await
        .is_err());
    assert!(!query.is_cancelled());
    cleanup.wait().await.unwrap();
    for panic in [false, true] {
        let cleanup = Arc::new(AudioCleanup::default());
        let query = PickerQuery::new(CancellationToken::new());
        assert!(query
            .inventory::<()>(cleanup.clone(), move |_, _| {
                if panic {
                    panic!("synthetic picker owner unwind");
                }
                Err(AudioCleanupError::Incomplete)
            })
            .await
            .is_err());
        assert!(cleanup.wait().await.is_err());
        assert!(!query.is_running());
    }
}
