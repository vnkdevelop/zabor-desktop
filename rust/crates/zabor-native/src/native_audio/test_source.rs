use super::*;
use std::sync::atomic::AtomicUsize;

#[derive(Default)]
pub struct Lifecycle {
    started: AtomicUsize,
    exited: AtomicUsize,
}

impl Lifecycle {
    pub fn started(&self) -> usize {
        self.started.load(Ordering::Acquire)
    }

    pub fn exited(&self) -> usize {
        self.exited.load(Ordering::Acquire)
    }
}

pub fn shared(
    controls: Arc<AudioControls>,
    stop: CancellationToken,
) -> (Arc<SharedAudio<NativeAudio>>, Arc<Lifecycle>) {
    let lifecycle = Arc::new(Lifecycle::default());
    let observed = lifecycle.clone();
    let audio = SharedAudio::new(controls.clone(), stop, move |demand, _, stop| {
        let controls = controls.clone();
        let lifecycle = observed.clone();
        Box::pin(async move {
            let guard = stop.clone().drop_guard();
            let runtime = tokio::runtime::Handle::current();
            let spawn = |lifecycle: Arc<Lifecycle>| {
                let runtime = runtime.clone();
                DeviceStream::test_worker(stop.child_token(), move |stop, ready| {
                    lifecycle.started.fetch_add(1, Ordering::AcqRel);
                    ready.send(Ok(())).map_err(|_| DeviceError)?;
                    runtime.block_on(stop.cancelled());
                    lifecycle.exited.fetch_add(1, Ordering::AcqRel);
                    Ok(())
                })
                .map_err(|_| MediaError::Microphone)
            };
            let capture = spawn(lifecycle.clone())?;
            let render = spawn(lifecycle)?;
            let audio = NativeAudio {
                source: audio_source(),
                mixer: Arc::new(AudioMixer::new(controls.clone())),
                demand,
                legacy_generation: None,
                processor_retry: Arc::new(ProcessorRetry::new(stop.clone())),
                stop,
                capture,
                render,
                controls,
            };
            guard.disarm();
            Ok(audio)
        })
    });
    (audio, lifecycle)
}
