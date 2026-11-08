use crate::audio_runtime::{AudioCleanup, AudioCleanupError};
use crate::media::MediaError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct PickerQuery {
    stop: CancellationToken,
    work_stop: CancellationToken,
    running: Arc<AtomicBool>,
}

impl PickerQuery {
    pub fn new(parent: CancellationToken) -> Self {
        let stop = parent.child_token();
        Self {
            work_stop: stop.child_token(),
            stop,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.stop.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.stop.is_cancelled()
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.work_stop.is_cancelled()
    }

    pub(crate) async fn inventory<T: Send + 'static>(
        &self,
        cleanup: Arc<AudioCleanup>,
        work: impl FnOnce(PickerQuery, oneshot::Sender<T>) -> Result<(), AudioCleanupError>
            + Send
            + 'static,
    ) -> Result<T, MediaError> {
        if self.is_stopped()
            || self
                .running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(MediaError::Backend);
        }
        let running = Running(self.running.clone());
        let prepared = cleanup.prepare_task().map_err(|_| MediaError::Backend)?;
        let mut cancellation = CancelOnDrop(Some(self.clone()));
        let query = self.clone();
        let (ready, received) = oneshot::channel();
        prepared.spawn_blocking_result(move || {
            let _running = running;
            if query.is_stopped() {
                return Ok(());
            }
            work(query, ready)
        });
        let result = tokio::select! {
            biased;
            _ = self.work_stop.cancelled() => return Err(MediaError::Backend),
            result = tokio::time::timeout(Duration::from_secs(5), received) => {
                result.map_err(|_| MediaError::Backend)?.map_err(|_| MediaError::Backend)?
            }
        };
        if self.is_stopped() {
            return Err(MediaError::Backend);
        }
        cancellation.0 = None;
        Ok(result)
    }
}

struct Running(Arc<AtomicBool>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct CancelOnDrop(Option<PickerQuery>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(query) = &self.0 {
            query.work_stop.cancel();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl PickerImage {
    pub fn from_bgra(
        bytes: &[u8],
        stride: u32,
        width: i32,
        height: i32,
    ) -> Result<Self, MediaError> {
        if width <= 0 || height <= 0 || width > 8192 || height > 8192 {
            return Err(MediaError::Backend);
        }
        let (width, height) = (width as u32, height as u32);
        let row = (width as usize).checked_mul(4).ok_or(MediaError::Backend)?;
        let required = (height as usize - 1)
            .checked_mul(stride as usize)
            .and_then(|offset| offset.checked_add(row))
            .ok_or(MediaError::Backend)?;
        if (stride as usize) < row || required > bytes.len() {
            return Err(MediaError::Backend);
        }
        let scale = (320.0 / f64::from(width))
            .min(180.0 / f64::from(height))
            .min(1.0);
        let target_width = (f64::from(width) * scale).round().max(1.0) as u32;
        let target_height = (f64::from(height) * scale).round().max(1.0) as u32;
        let mut rgba = vec![0; target_width as usize * target_height as usize * 4];
        for y in 0..target_height {
            let source_y = u64::from(y) * u64::from(height) / u64::from(target_height);
            for x in 0..target_width {
                let source_x = u64::from(x) * u64::from(width) / u64::from(target_width);
                let offset = source_y as usize * stride as usize + source_x as usize * 4;
                let target = (y as usize * target_width as usize + x as usize) * 4;
                rgba[target..target + 4].copy_from_slice(&[
                    bytes[offset + 2],
                    bytes[offset + 1],
                    bytes[offset],
                    255,
                ]);
            }
        }
        Ok(Self {
            width: target_width,
            height: target_height,
            rgba,
        })
    }
}

#[cfg(test)]
mod tests;
