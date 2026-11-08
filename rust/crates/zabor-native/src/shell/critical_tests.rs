use super::{queue_control_command, Command, CommandQueue, UiEvent};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use zabor_native::critical_intents::{CriticalAction, CriticalIntents, IntentScope};

#[tokio::test]
async fn critical_leave_bypasses_full_queue_and_old_voice_work_cannot_resurrect() {
    let intents = Arc::new(CriticalIntents::default());
    let (sender, mut receiver) = mpsc::channel(16);
    let queue = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    for _ in 0..16 {
        assert!(queue.try_send(Command::Join("old-channel".into())).is_ok());
    }
    assert!(queue.try_send(Command::Call("blocked".into())).is_err());
    assert!(queue.try_send(Command::Leave).is_ok());
    assert_eq!(intents.next().await, CriticalAction::Leave);
    for _ in 0..16 {
        let queued = receiver.recv().await.unwrap();
        assert!(!intents.is_current(queued.ticket, queued.command.scope()));
    }
    assert!(queue.try_send(Command::Call("fresh-peer".into())).is_ok());
    let queued = receiver.recv().await.unwrap();
    assert!(intents.is_current(queued.ticket, queued.command.scope()));
}

#[tokio::test]
async fn critical_logout_dominates_leave_and_rejects_every_queued_old_owner_action() {
    let intents = Arc::new(CriticalIntents::default());
    let (sender, mut receiver) = mpsc::channel(16);
    let queue = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    for _ in 0..16 {
        assert!(queue
            .try_send(Command::Modal {
                kind: 1,
                value: "local".into()
            })
            .is_ok());
    }
    assert!(queue.send(Command::Logout).await.is_ok());
    assert!(queue.try_send(Command::Leave).is_ok());
    assert_eq!(intents.next().await, CriticalAction::Logout);
    for _ in 0..16 {
        let queued = receiver.recv().await.unwrap();
        assert!(!intents.is_current(queued.ticket, queued.command.scope()));
    }
    assert!(queue.try_send(Command::Captcha).is_ok());
    let queued = receiver.recv().await.unwrap();
    assert!(intents.is_current(queued.ticket, IntentScope::Session));
}

#[tokio::test]
async fn ui_dispatch_rejects_stale_nested_voice_and_owner_completions() {
    let intents = CriticalIntents::default();
    let old = intents.ticket();
    let voice = UiEvent::Calling("old-peer".into())
        .guarded(old, IntentScope::Voice)
        .guarded(old, IntentScope::Session);
    intents.request(CriticalAction::Leave);
    assert!(voice.for_current(&intents).is_none());
    assert!(UiEvent::Connected
        .guarded(old, IntentScope::Session)
        .for_current(&intents)
        .is_some());
    assert_eq!(intents.next().await, CriticalAction::Leave);
    let fresh = intents.ticket();
    assert!(UiEvent::Calling("fresh".into())
        .guarded(fresh, IntentScope::Voice)
        .for_current(&intents)
        .is_some());
    let login = UiEvent::SignedIn(Box::default()).guarded(fresh, IntentScope::Session);
    intents.request(CriticalAction::Logout);
    assert_eq!(intents.next().await, CriticalAction::Logout);
    assert!(login.for_current(&intents).is_none());
}

#[tokio::test]
async fn closed_queue_does_not_report_a_delivered_critical_command() {
    let intents = Arc::new(CriticalIntents::default());
    let old = intents.ticket();
    let (sender, receiver) = mpsc::channel(16);
    let queue = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    drop(receiver);
    assert!(queue.try_send(Command::Leave).is_err());
    assert!(queue.send(Command::Logout).await.is_err());
    assert_eq!(intents.ticket(), old);
}

#[tokio::test]
async fn interrupted_call_teardown_keeps_endcall_hint_after_future_is_dropped() {
    let intents = CriticalIntents::default();
    let ticket = intents.ticket();
    let mut abandoned = false;
    let future = async {
        abandoned = true;
        intents.request(CriticalAction::Leave);
        std::future::pending::<()>().await;
        abandoned = false;
    };
    assert_eq!(intents.run(ticket, IntentScope::Voice, future).await, None);
    assert!(abandoned);
    assert_eq!(intents.next().await, CriticalAction::Leave);
}

#[tokio::test]
async fn async_queue_backpressure_preserves_the_original_owner_ticket() {
    use std::future::Future;
    use std::task::Poll;

    let intents = Arc::new(CriticalIntents::default());
    let (sender, mut receiver) = mpsc::channel(16);
    let queue = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    for _ in 0..16 {
        assert!(queue.try_send(Command::Audio).is_ok());
    }
    let future = queue.send(Command::Captcha);
    tokio::pin!(future);
    std::future::poll_fn(|context| {
        assert!(future.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    intents.request(CriticalAction::Logout);
    assert_eq!(intents.next().await, CriticalAction::Logout);
    for _ in 0..16 {
        receiver.recv().await.unwrap();
    }
    assert!(future.await.is_ok());
    let queued = receiver.recv().await.unwrap();
    assert!(!intents.is_current(queued.ticket, IntentScope::Session));
}

#[tokio::test]
async fn camera_inventory_queue_and_late_ui_completion_keep_the_original_voice_ticket() {
    let intents = Arc::new(CriticalIntents::default());
    let ticket = intents.ticket();
    let (sender, mut receiver) = mpsc::channel(16);
    let queue = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    let query = zabor_native::picker::PickerQuery::new(tokio_util::sync::CancellationToken::new());
    assert!(queue.try_send(Command::ListCameras(query.clone())).is_ok());
    let queued = receiver.recv().await.unwrap();
    assert_eq!(queued.ticket, ticket);
    assert_eq!(queued.command.scope(), IntentScope::Voice);
    let event = UiEvent::Cameras(query, Some(Vec::new())).guarded(ticket, IntentScope::Voice);
    intents.request(CriticalAction::Leave);
    assert!(!intents.is_current(queued.ticket, queued.command.scope()));
    assert!(event.for_current(&intents).is_none());
    assert!(intents.is_current(ticket, IntentScope::Session));
}

#[tokio::test]
async fn stream_audio_selection_and_focus_commands_cannot_escape_the_original_voice_epoch() {
    let intents = Arc::new(CriticalIntents::default());
    let (sender, mut receiver) = mpsc::channel(16);
    let queue = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    assert!(queue
        .try_send(Command::StartShare(
            "window:98765".into(),
            "high".into(),
            false,
            false
        ))
        .is_ok());
    assert!(queue
        .try_send(Command::FocusStreamAudio("peer".into()))
        .is_ok());
    let start = receiver.recv().await.unwrap();
    let focus = receiver.recv().await.unwrap();
    assert!(matches!(
        start.command,
        Command::StartShare(_, _, false, false)
    ));
    assert_eq!(start.command.scope(), IntentScope::Voice);
    assert_eq!(focus.command.scope(), IntentScope::Voice);
    intents.request(CriticalAction::Leave);
    assert!(!intents.is_current(start.ticket, start.command.scope()));
    assert!(!intents.is_current(focus.ticket, focus.command.scope()));
}

#[tokio::test]
async fn desktop_inventory_and_preview_callbacks_keep_voice_and_picker_identity_without_renewal() {
    use zabor_native::picker::{PickerImage, PickerQuery};
    use zabor_native::screen::SourceKind;
    let intents = Arc::new(CriticalIntents::default());
    let original = intents.ticket();
    let query = PickerQuery::new(intents.voice_token(original));
    let (sender, mut receiver) = mpsc::channel(16);
    let queue = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    assert!(queue
        .try_send(Command::ListScreens(query.clone(), 42, SourceKind::Window))
        .is_ok());
    let queued = receiver.recv().await.unwrap();
    assert_eq!(queued.ticket, original);
    assert_eq!(queued.command.scope(), IntentScope::Voice);
    let image = PickerImage {
        width: 1,
        height: 1,
        rgba: vec![0, 0, 0, 255],
    };
    let event = UiEvent::SourcePreview(query.clone(), "window:43".into(), Some(image.clone()))
        .guarded(original, IntentScope::Voice);
    query.cancel();
    assert!(event.for_current(&intents).is_none());
    assert!(intents.is_current(original, IntentScope::Voice));
    let query = PickerQuery::new(intents.voice_token(original));
    let inventory =
        UiEvent::Screens(query.clone(), Some(Vec::new())).guarded(original, IntentScope::Voice);
    let preview = UiEvent::SourcePreview(query.clone(), "screen:43".into(), Some(image))
        .guarded(original, IntentScope::Voice);
    intents.request(CriticalAction::Leave);
    assert!(query.is_cancelled());
    assert!(inventory.for_current(&intents).is_none());
    assert!(preview.for_current(&intents).is_none());
    assert!(intents.is_current(original, IntentScope::Session));
}

#[tokio::test]
async fn independent_picker_work_stops_on_original_leave_or_logout_without_cancelling_session_on_leave(
) {
    for action in [CriticalAction::Leave, CriticalAction::Logout] {
        let intents = CriticalIntents::default();
        let original = intents.ticket();
        let session = intents.session_token(original);
        let voice = intents.voice_token(original);
        assert!(!voice.is_cancelled());
        let child = voice.child_token();
        child.cancel();
        assert!(!voice.is_cancelled());
        intents.request(action);
        assert!(voice.is_cancelled());
        assert!(intents.voice_token(original).is_cancelled());
        assert_eq!(session.is_cancelled(), action == CriticalAction::Logout);
        assert_eq!(intents.next().await, action);
        assert!(!intents.voice_token(intents.ticket()).is_cancelled());
        assert!(intents.voice_token(original).is_cancelled());
    }
}

#[tokio::test]
async fn stream_maintenance_keeps_original_voice_admission_and_cannot_run_after_leave() {
    let intents = Arc::new(CriticalIntents::default());
    let original = intents.ticket();
    let (sender, mut receiver) = mpsc::channel(16);
    let queue = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    assert!(queue.try_send(Command::MaintainStreams).is_ok());
    let queued = receiver.recv().await.unwrap();
    assert_eq!(queued.ticket, original);
    assert_eq!(queued.command.scope(), IntentScope::Voice);
    intents.request(CriticalAction::Leave);
    assert!(!intents.is_current(queued.ticket, queued.command.scope()));
    assert!(intents.is_current(original, IntentScope::Session));
}

#[tokio::test]
async fn remote_release_and_revoke_keep_original_voice_tickets_while_waiting_for_queue_space() {
    let intents = Arc::new(CriticalIntents::default());
    let (sender, mut receiver) = mpsc::channel(1);
    let queue = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    assert!(queue.try_send(Command::MaintainStreams).is_ok());
    let original = intents.ticket();
    queue_control_command(
        &queue,
        &tokio::runtime::Handle::current(),
        Command::ReleaseControl,
    );
    intents.request(CriticalAction::Leave);
    assert_eq!(receiver.recv().await.unwrap().ticket, original);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), receiver.recv())
            .await
            .is_err()
    );
    assert!(matches!(Command::RevokeControl.scope(), IntentScope::Voice));
    assert!(matches!(
        Command::ReleaseControl.scope(),
        IntentScope::Voice
    ));
}
