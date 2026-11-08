use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const SAMPLE_RATE: u32 = 48000;
pub const CHANNELS: u32 = 2;
pub const MAX_PACKET_FRAMES: usize = 9600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Screen,
    Window(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    process_id: u32,
    include_tree: bool,
}

impl Target {
    pub fn process_id(self) -> u32 {
        self.process_id
    }

    pub fn include_tree(self) -> bool {
        self.include_tree
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    #[error("isolated stream audio is unavailable")]
    Unavailable,
    #[error("isolated stream audio cleanup is incomplete")]
    Cleanup,
}

pub struct Packet {
    pub samples: Vec<f32>,
    pub discontinuity: bool,
}

fn resolve(
    source: Source,
    current_process: u32,
    window_process: impl FnOnce(u64) -> Result<u32, CaptureError>,
) -> Result<Target, CaptureError> {
    let (process_id, include_tree) = match source {
        Source::Screen => (current_process, false),
        Source::Window(0) => return Err(CaptureError::Unavailable),
        Source::Window(window) => (window_process(window)?, true),
    };
    if process_id == 0 {
        return Err(CaptureError::Unavailable);
    }
    Ok(Target {
        process_id,
        include_tree,
    })
}

fn decode(bytes: &[u8], frames: usize, silent: bool) -> Result<Vec<f32>, CaptureError> {
    if frames > MAX_PACKET_FRAMES {
        return Err(CaptureError::Unavailable);
    }
    let samples = frames * CHANNELS as usize;
    if silent {
        return Ok(vec![0.0; samples]);
    }
    if bytes.len() != samples * 4 {
        return Err(CaptureError::Unavailable);
    }
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| {
            let sample = f32::from_le_bytes(*bytes);
            if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        })
        .collect())
}

trait Backend {
    fn packet(&mut self) -> Result<Option<Packet>, CaptureError>;
    fn finish(&mut self) -> Result<(), CaptureError>;
}

fn drive<B: Backend>(
    mut backend: B,
    target: Target,
    cancelled: impl Fn() -> bool,
    ready: impl FnOnce(Target) -> bool,
    mut deliver: impl FnMut(Packet) -> bool,
) -> Result<(), CaptureError> {
    let result = (|| {
        if cancelled() || !ready(target) {
            return Ok(());
        }
        while !cancelled() {
            if let Some(packet) = backend.packet()? {
                if cancelled() || !deliver(packet) {
                    break;
                }
            }
            std::thread::park_timeout(std::time::Duration::from_millis(5));
        }
        Ok(())
    })();
    backend.finish()?;
    result
}

pub fn run(
    source: Source,
    cancelled: impl Fn() -> bool,
    ready: impl FnOnce(Target) -> bool,
    deliver: impl FnMut(Packet) -> bool,
) -> Result<(), CaptureError> {
    if cancelled() {
        return Ok(());
    }
    #[cfg(windows)]
    {
        let target = resolve(source, std::process::id(), native::window_process)?;
        let cleanup_failed = Arc::new(AtomicBool::new(false));
        let result = native::Backend::open(target, cleanup_failed.clone(), &cancelled)
            .and_then(|backend| drive(backend, target, cancelled, ready, deliver));
        if cleanup_failed.load(Ordering::Acquire) {
            Err(CaptureError::Cleanup)
        } else {
            result
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (source, ready, deliver);
        Err(CaptureError::Unavailable)
    }
}

#[cfg(windows)]
mod native;

#[cfg(feature = "test-source")]
pub fn run_synthetic(
    source: Source,
    mode: u8,
    delay_ms: u32,
    cancelled: impl Fn() -> bool,
    ready: impl FnOnce(Target) -> bool,
    deliver: impl FnMut(Packet) -> bool,
) -> Result<(), CaptureError> {
    if mode > 5 || delay_ms > 1000 {
        return Err(CaptureError::Unavailable);
    }
    if cancelled() {
        return Ok(());
    }
    let target = resolve(source, 4242, |_| Ok(5353))?;
    if mode == 1 {
        std::thread::sleep(std::time::Duration::from_millis(u64::from(delay_ms)));
    }
    if mode == 3 {
        return Err(CaptureError::Unavailable);
    }
    drive(
        Synthetic {
            mode,
            delay_ms,
            finished: false,
            count: 0,
            owner: std::thread::current().id(),
        },
        target,
        cancelled,
        ready,
        deliver,
    )
}

#[cfg(feature = "test-source")]
struct Synthetic {
    mode: u8,
    delay_ms: u32,
    finished: bool,
    count: usize,
    owner: std::thread::ThreadId,
}

#[cfg(feature = "test-source")]
impl Backend for Synthetic {
    fn packet(&mut self) -> Result<Option<Packet>, CaptureError> {
        assert_eq!(self.owner, std::thread::current().id());
        self.count += 1;
        if self.mode == 4 && self.count > 1 {
            return Err(CaptureError::Unavailable);
        }
        Ok(Some(Packet {
            samples: (0..480).flat_map(|_| [-0.25, 0.5]).collect(),
            discontinuity: false,
        }))
    }

    fn finish(&mut self) -> Result<(), CaptureError> {
        assert_eq!(self.owner, std::thread::current().id());
        if !self.finished {
            if self.mode == 2 {
                std::thread::sleep(std::time::Duration::from_millis(u64::from(self.delay_ms)));
            }
            self.finished = true;
        }
        if self.mode == 5 {
            Err(CaptureError::Cleanup)
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "test-source")]
impl Drop for Synthetic {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[cfg(test)]
mod tests;
