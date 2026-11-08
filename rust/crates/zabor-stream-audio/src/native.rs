use super::{decode, CaptureError, Packet, Target, MAX_PACKET_FRAMES};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use wasapi::{AudioClient, Direction, Handle, SampleType, StreamMode, WaveFormat};
use windows::Win32::Foundation::HWND;
use windows::Win32::Media::Audio::{
    IAudioCaptureClient, AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY, AUDCLNT_BUFFERFLAGS_SILENT,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow};

pub(super) fn window_process(window: u64) -> Result<u32, CaptureError> {
    let value = isize::try_from(window).map_err(|_| CaptureError::Unavailable)?;
    let hwnd = HWND(value as *mut std::ffi::c_void);
    let mut process = 0;
    let valid = unsafe {
        IsWindow(Some(hwnd)).as_bool() && GetWindowThreadProcessId(hwnd, Some(&mut process)) != 0
    };
    if !valid || process == 0 {
        return Err(CaptureError::Unavailable);
    }
    Ok(process)
}

pub(super) struct Backend {
    client: Option<AudioClient>,
    capture: Option<IAudioCaptureClient>,
    event: Option<Handle>,
    apartment: bool,
    start_attempted: bool,
    cleanup_failed: Arc<AtomicBool>,
    owner: std::thread::ThreadId,
    apartment_bound: PhantomData<Rc<()>>,
}

impl Backend {
    pub(super) fn open(
        target: Target,
        cleanup_failed: Arc<AtomicBool>,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self, CaptureError> {
        if cancelled() {
            return Err(CaptureError::Unavailable);
        }
        wasapi::initialize_mta()
            .ok()
            .map_err(|_| CaptureError::Unavailable)?;
        let mut owner = Self {
            client: None,
            capture: None,
            event: None,
            apartment: true,
            start_attempted: false,
            cleanup_failed,
            owner: std::thread::current().id(),
            apartment_bound: PhantomData,
        };
        owner.client = Some(
            AudioClient::new_application_loopback_client(
                target.process_id(),
                target.include_tree(),
            )
            .map_err(|_| CaptureError::Unavailable)?,
        );
        if cancelled() {
            return Err(CaptureError::Unavailable);
        }
        let client = owner.client.as_mut().unwrap();
        let format = WaveFormat::new(32, 32, &SampleType::Float, 48000, 2, None);
        client
            .initialize_client(
                &format,
                &Direction::Capture,
                &StreamMode::EventsShared {
                    autoconvert: true,
                    buffer_duration_hns: 0,
                },
            )
            .map_err(|_| CaptureError::Unavailable)?;
        owner.event = Some(
            client
                .set_get_eventhandle()
                .map_err(|_| CaptureError::Unavailable)?,
        );
        owner.capture = Some(
            unsafe { client.as_iaudioclient().GetService::<IAudioCaptureClient>() }
                .map_err(|_| CaptureError::Unavailable)?,
        );
        if cancelled() {
            return Err(CaptureError::Unavailable);
        }
        owner.start_attempted = true;
        client
            .start_stream()
            .map_err(|_| CaptureError::Unavailable)?;
        Ok(owner)
    }

    fn release(&mut self) -> Result<(), CaptureError> {
        if self.owner != std::thread::current().id() {
            self.cleanup_failed.store(true, Ordering::Release);
            return Err(CaptureError::Cleanup);
        }
        if self.start_attempted {
            self.start_attempted = false;
            if self.client.as_ref().unwrap().stop_stream().is_err() {
                self.cleanup_failed.store(true, Ordering::Release);
            }
        }
        self.capture = None;
        self.event = None;
        self.client = None;
        if self.apartment {
            self.apartment = false;
            wasapi::deinitialize();
        }
        if self.cleanup_failed.load(Ordering::Acquire) {
            Err(CaptureError::Cleanup)
        } else {
            Ok(())
        }
    }
}

struct Buffer<'a> {
    capture: &'a IAudioCaptureClient,
    frames: u32,
    released: bool,
    cleanup_failed: &'a AtomicBool,
}

impl Buffer<'_> {
    fn release(&mut self) -> Result<(), CaptureError> {
        self.released = true;
        if unsafe { self.capture.ReleaseBuffer(self.frames) }.is_err() {
            self.cleanup_failed.store(true, Ordering::Release);
            return Err(CaptureError::Cleanup);
        }
        Ok(())
    }
}

impl Drop for Buffer<'_> {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.release();
        }
    }
}

impl super::Backend for Backend {
    fn packet(&mut self) -> Result<Option<Packet>, CaptureError> {
        if self.owner != std::thread::current().id() {
            return Err(CaptureError::Cleanup);
        }
        let capture = self.capture.as_ref().unwrap();
        let next = unsafe { capture.GetNextPacketSize() }.map_err(|_| CaptureError::Unavailable)?;
        if next == 0 {
            if let Some(event) = &self.event {
                let _ = event.wait_for_event(5);
            }
            return Ok(None);
        }
        let mut pointer = std::ptr::null_mut();
        let mut frames = 0;
        let mut flags = 0;
        unsafe { capture.GetBuffer(&mut pointer, &mut frames, &mut flags, None, None) }
            .map_err(|_| CaptureError::Unavailable)?;
        let mut buffer = Buffer {
            capture,
            frames,
            released: false,
            cleanup_failed: &self.cleanup_failed,
        };
        let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
        let result = if frames as usize > MAX_PACKET_FRAMES
            || (!silent && frames > 0 && pointer.is_null())
        {
            Err(CaptureError::Unavailable)
        } else {
            let bytes = if silent || frames == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(pointer, frames as usize * 8) }
            };
            decode(bytes, frames as usize, silent).map(|samples| Packet {
                samples,
                discontinuity: flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0,
            })
        };
        buffer.release()?;
        result.map(Some)
    }

    fn finish(&mut self) -> Result<(), CaptureError> {
        self.release()
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
