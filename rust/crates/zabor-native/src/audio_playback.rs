use crate::audio_mix::{AudioLane, AudioMixer};
use crate::media::{MediaError, PeerLifetime};
use futures_util::StreamExt;
use libwebrtc::audio_stream::native::{NativeAudioStream, NativeAudioStreamOptions};
use libwebrtc::audio_track::RtcAudioTrack;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

struct Subscription {
    id: String,
    lane: Arc<AudioLane>,
    mixer: Arc<AudioMixer>,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.stop.cancel();
        self.mixer.remove_lane(&self.id, &self.lane);
        self.task.abort();
    }
}

pub struct AudioPlayback {
    mixer: Arc<AudioMixer>,
    subscriptions: BTreeMap<(String, String), Subscription>,
}

impl AudioPlayback {
    pub fn new(mixer: Arc<AudioMixer>) -> Self {
        Self {
            mixer,
            subscriptions: BTreeMap::new(),
        }
    }
    pub fn attach(
        &mut self,
        peer: &str,
        track: RtcAudioTrack,
        lifetime: PeerLifetime,
        stream: bool,
    ) -> Result<(), MediaError> {
        self.retain_alive();
        if !lifetime.is_alive() {
            return Err(MediaError::UnauthorizedPeer);
        }
        let key = (peer.to_owned(), track.id());
        if self.subscriptions.len() >= 128 && !self.subscriptions.contains_key(&key) {
            return Err(MediaError::Backend);
        }
        self.subscriptions.remove(&key);
        let id = serde_json::to_string(&key).map_err(|_| MediaError::Backend)?;
        let lane = self
            .mixer
            .add(id.clone(), peer, lifetime.clone(), stream)
            .ok_or(MediaError::Backend)?;
        let mut audio = NativeAudioStream::with_options(
            track,
            48000,
            1,
            NativeAudioStreamOptions {
                queue_size_frames: Some(4),
            },
        );
        let stop = CancellationToken::new();
        let cancel = stop.clone();
        let output = lane.clone();
        let task = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    _ = tick.tick() => { if !lifetime.is_alive() { break; } },
                    frame = audio.next() => {
                        let Some(frame) = frame else { break; };
                        if !lifetime.is_alive() || cancel.is_cancelled() { break; }
                        if frame.sample_rate == 48000 && frame.num_channels == 1 && frame.samples_per_channel as usize == frame.data.len() { output.push(&frame.data); }
                    }
                }
            }
            output.stop();
            audio.close();
        });
        self.subscriptions.insert(
            key,
            Subscription {
                id,
                lane,
                mixer: self.mixer.clone(),
                stop,
                task,
            },
        );
        Ok(())
    }
    pub fn retain_alive(&mut self) {
        self.subscriptions.retain(|_, subscription| {
            !subscription.task.is_finished() && subscription.lane.is_active()
        });
    }
    pub fn clear(&mut self) {
        self.subscriptions.clear();
    }
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }
}
