use super::{Command, CommandQueue, UiEvent};
use std::sync::Arc;
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
