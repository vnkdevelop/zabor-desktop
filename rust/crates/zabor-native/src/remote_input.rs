use crate::audio_runtime::{AudioCleanup, AudioCleanupError};
use crate::media::{ChannelLease, MediaError, PeerLifetime};
use std::sync::{mpsc, Arc};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use zabor_input::{CaptureLease, Event, InputError, Packet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputState {
    Starting,
    Ready,
    HostActive,
    HostIdle,
    Revoked,
    Ended,
}

pub(crate) struct RemoteInput {
    sender: mpsc::SyncSender<Packet>,
    stop: CancellationToken,
    state: watch::Receiver<InputState>,
}

impl RemoteInput {
    pub async fn start(
        cleanup: Arc<AudioCleanup>,
        geometry: Arc<CaptureLease>,
        voice: CancellationToken,
        lifetime: PeerLifetime,
        channel: ChannelLease,
    ) -> Result<Self, MediaError> {
        Self::start_with(cleanup, voice, move |receiver, stop, status| {
            zabor_input::run(
                geometry,
                receiver,
                || stop.is_cancelled() || !lifetime.is_alive() || !channel.is_alive(),
                |event| {
                    status
                        .send(match event {
                            Event::Ready => InputState::Ready,
                            Event::HostActive => InputState::HostActive,
                            Event::HostIdle => InputState::HostIdle,
                            Event::Revoked => InputState::Revoked,
                        })
                        .is_ok()
                },
            )
        })
        .await
    }

    pub(crate) async fn start_with(
        cleanup: Arc<AudioCleanup>,
        voice: CancellationToken,
        run: impl FnOnce(
                mpsc::Receiver<Packet>,
                CancellationToken,
                watch::Sender<InputState>,
            ) -> Result<(), InputError>
            + Send
            + 'static,
    ) -> Result<Self, MediaError> {
        let prepared = cleanup.prepare_task().map_err(|_| MediaError::Backend)?;
        let stop = voice.child_token();
        let guard = stop.clone().drop_guard();
        let (sender, receiver) = mpsc::sync_channel(128);
        let (status, mut state) = watch::channel(InputState::Starting);
        let worker_stop = stop.clone();
        prepared.spawn_blocking_result(move || {
            let result = if worker_stop.is_cancelled() {
                Ok(())
            } else {
                run(receiver, worker_stop, status.clone())
            };
            status.send_replace(if result.is_err() {
                InputState::Revoked
            } else {
                InputState::Ended
            });
            if result == Err(InputError::Cleanup) {
                Err(AudioCleanupError::Incomplete)
            } else {
                Ok(())
            }
        });
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(MediaError::Backend),
            result = tokio::time::timeout(std::time::Duration::from_secs(2), state.wait_for(|state| *state != InputState::Starting)) => {
                if !matches!(result, Ok(Ok(state)) if matches!(*state, InputState::Ready | InputState::HostActive | InputState::HostIdle)) { return Err(MediaError::Backend); }
            }
        }
        if stop.is_cancelled() {
            return Err(MediaError::Backend);
        }
        guard.disarm();
        Ok(Self {
            sender,
            stop,
            state,
        })
    }

    pub fn feed(&self, packet: Packet) -> bool {
        if self.stop.is_cancelled() {
            return false;
        }
        if self.sender.try_send(packet).is_err() {
            self.stop.cancel();
            return false;
        }
        true
    }

    pub fn state(&self) -> InputState {
        if self.state.has_changed().is_err() {
            InputState::Revoked
        } else {
            *self.state.borrow()
        }
    }
    pub fn stop(&self) {
        self.stop.cancel();
    }
}

impl Drop for RemoteInput {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn original_voice_revocation_and_drop_retire_registered_input_without_blocking_the_caller(
    ) {
        let cleanup = Arc::new(AudioCleanup::default());
        let voice = CancellationToken::new();
        let finished = Arc::new(AtomicBool::new(false));
        let owner_finished = finished.clone();
        let input =
            RemoteInput::start_with(cleanup.clone(), voice.clone(), move |_, stop, state| {
                state.send_replace(InputState::Ready);
                while !stop.is_cancelled() {
                    std::thread::park_timeout(std::time::Duration::from_millis(1));
                }
                owner_finished.store(true, Ordering::Release);
                Ok(())
            })
            .await
            .unwrap();
        voice.cancel();
        drop(input);
        cleanup.wait().await.unwrap();
        assert!(finished.load(Ordering::Acquire));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn held_owner_cleanup_and_failure_remain_observed_after_public_handle_drop() {
        let cleanup = Arc::new(AudioCleanup::default());
        let (release, blocked) = mpsc::sync_channel(1);
        let input = RemoteInput::start_with(
            cleanup.clone(),
            CancellationToken::new(),
            move |_, stop, state| {
                state.send_replace(InputState::Ready);
                while !stop.is_cancelled() {
                    std::thread::park_timeout(std::time::Duration::from_millis(1));
                }
                blocked.recv().unwrap();
                Err(InputError::Cleanup)
            },
        )
        .await
        .unwrap();
        drop(input);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), cleanup.wait())
                .await
                .is_err()
        );
        release.send(()).unwrap();
        assert!(matches!(
            cleanup.wait().await,
            Err(AudioCleanupError::Incomplete)
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn input_queue_overflow_cancels_the_original_owner_instead_of_dropping_a_release() {
        let cleanup = Arc::new(AudioCleanup::default());
        let input = RemoteInput::start_with(
            cleanup.clone(),
            CancellationToken::new(),
            move |_, stop, state| {
                state.send_replace(InputState::Ready);
                while !stop.is_cancelled() {
                    std::thread::park_timeout(std::time::Duration::from_millis(1));
                }
                Ok(())
            },
        )
        .await
        .unwrap();
        for _ in 0..128 {
            assert!(input.feed(Packet::Signal(zabor_input::Signal::Ping)));
        }
        assert!(!input.feed(Packet::Mouse {
            button: 0,
            down: false
        }));
        assert!(input.stop.is_cancelled());
        drop(input);
        cleanup.wait().await.unwrap();
    }
}
