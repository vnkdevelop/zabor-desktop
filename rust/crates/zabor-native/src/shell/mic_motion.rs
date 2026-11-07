use crate::{MicTestView, NativeApp};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

fn save_panel(ui: &NativeApp, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let pixels = ui.window().take_snapshot()?;
    let scale = ui.window().scale_factor();
    let x = ((pixels.width() as f32 - 156.0 * scale) / 2.0)
        .round()
        .max(0.0) as usize;
    let y = (ui.get_mic_test_panel_y() * scale).round().max(0.0) as usize;
    let width = (156.0 * scale).round() as usize;
    let height = (84.0 * scale).ceil() as usize;
    let stride = pixels.width() as usize * 4;
    let mut crop = vec![0_u8; width * height * 4];
    for row in 0..height {
        if y + row < pixels.height() as usize && x + width <= pixels.width() as usize {
            crop[row * width * 4..(row + 1) * width * 4].copy_from_slice(
                &pixels.as_bytes()
                    [(y + row) * stride + x * 4..(y + row) * stride + (x + width) * 4],
            );
        }
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&crop)?;
    writer.finish()?;
    Ok(())
}

pub(super) fn start(
    ui: &NativeApp,
    timer: &Timer,
    output: PathBuf,
    error: Rc<RefCell<Option<String>>>,
    capture_frames: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if output.exists() {
        return Err("Motion evidence directory already exists".into());
    }
    std::fs::create_dir_all(&output)?;
    let weak = ui.as_weak();
    ui.on_mic_test_seek(move |seconds| {
        if let Some(ui) = weak.upgrade() {
            let mut test = ui.get_mic_test();
            test.position = seconds.clamp(0.0, 4.95);
            ui.set_mic_test(test);
        }
    });
    let weak = ui.as_weak();
    let started = Instant::now();
    let mut phase_started = started;
    let mut phase = 0_u32;
    let mut frame = 0_u32;
    let mut phase_frame = 0_u32;
    let mut peak = 0.0_f32;
    timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        let Some(ui) = weak.upgrade() else { return; };
        let elapsed = phase_started.elapsed().as_millis() as u64;
        if phase == 1 {
            let mut test = ui.get_mic_test();
            test.position = if phase_frame.is_multiple_of(2) { 1.5 } else { 1.501 };
            ui.set_mic_test(test);
        }
        let scale = ui.get_mic_test_thumb_scale();
        peak = peak.max(scale);
        println!("{}", serde_json::json!({"nativeMicMotionFrame":{
            "frame":frame,"phase":phase,"elapsedMs":elapsed,"totalMs":started.elapsed().as_millis(),
            "scale":scale,"animating":ui.get_mic_test_thumb_animating(),
            "position":ui.get_mic_test().position,"stage":ui.get_mic_test().stage.as_str(),
            "panelY":ui.get_mic_test_panel_y(),"reducedMotion":ui.get_reduced_motion(),
            "physicalDevices":false}}));
        let pointer = |down, x| {
            let position = slint::LogicalPosition::new(x, ui.get_mic_test_panel_y() + 18.0);
            ui.window().dispatch_event(if down {
                WindowEvent::PointerPressed { position, button: PointerEventButton::Left }
            } else {
                WindowEvent::PointerReleased { position, button: PointerEventButton::Left }
            });
        };
        let result = (|| -> Result<bool, Box<dyn std::error::Error>> {
            if capture_frames && [0, 1, 4, 10].contains(&phase_frame) {
                let path = output.join(format!("phase-{phase}-frame-{phase_frame}.png"));
                save_panel(&ui, &path)?;
            }
            let duration = match phase {
                0 => 500, 1 => 1950, 2 => 120, 3 => 80, 4 | 5 => 1320,
                6 => 80, 7..=10 | 12 => 50, 11 => 200, _ => return Err("Invalid motion phase".into()),
            };
            if elapsed < duration { return Ok(false); }
            if capture_frames && [1, 4, 5].contains(&phase) && ui.get_mic_test_thumb_animating() {
                if elapsed > 30_000 { return Err("PNG capture spring completion timed out".into()); }
                return Ok(false);
            }
            match phase {
                0 | 10 => ui.set_mic_test(MicTestView {
                    stage: "ready".into(), issue: "".into(), playing: false, duration: 5.0, position: 1.5,
                }),
                1 => {
                    if (!capture_frames && peak < 1.24) || (scale - 1.0).abs() > 0.0001 || ui.get_mic_test_thumb_animating() {
                        return Err("Ready pop did not overshoot and settle once".into());
                    }
                    let mut test = ui.get_mic_test();
                    test.position = 1.5;
                    ui.set_mic_test(test);
                    pointer(true, 548.0);
                }
                2 => {
                    if ui.get_mic_test().position != 1.5 { return Err("Motion scrub committed before release".into()); }
                    pointer(false, 580.0);
                }
                3 => {
                    if (ui.get_mic_test().position - 69.0 / 104.0 * 5.0).abs() > 0.001 {
                        return Err("Motion release-coordinate seek changed".into());
                    }
                    pointer(true, 548.0);
                }
                4 => {
                    if (scale - 1.5).abs() > 0.0001 || ui.get_mic_test_thumb_animating() {
                        return Err("Interrupted press did not settle at 1.5".into());
                    }
                    pointer(false, 548.0);
                }
                5 => {
                    if (scale - 1.0).abs() > 0.0001 || ui.get_mic_test_thumb_animating() {
                        return Err("Release spring did not settle at 1".into());
                    }
                    pointer(true, 548.0);
                }
                6 => ui.set_reduced_motion(true),
                7 => {
                    if (scale - 1.5).abs() > 0.0001 || ui.get_mic_test_thumb_animating() {
                        return Err("Reduced motion did not snap the in-flight press".into());
                    }
                    pointer(false, 548.0);
                }
                8 => {
                    if (scale - 1.0).abs() > 0.0001 || ui.get_mic_test_thumb_animating() {
                        return Err("Reduced-motion release did not remain immediate".into());
                    }
                    ui.set_reduced_motion(false);
                    ui.set_settings_tab(0);
                }
                9 => {
                    if ui.get_mic_test_thumb_animating() { return Err("Hidden panel retained thumb animation".into()); }
                    ui.set_settings_tab(1);
                    ui.set_mic_test(MicTestView { stage: "idle".into(), ..Default::default() });
                }
                11 => ui.set_mic_test(MicTestView { stage: "arming".into(), ..Default::default() }),
                12 => {
                    if ui.get_mic_test_thumb_animating() || (scale - 1.0).abs() > 0.0001 {
                        return Err("Retake did not cancel the old ready pop".into());
                    }
                    println!("{}", serde_json::json!({"nativeMicMotionCheck":if capture_frames {"captured"} else {"passed"},"frames":frame + 1,
                        "phases":13,"physicalDevices":false,"nativeClientReady":false,"micTestUiReady":false}));
                    let _ = ui.hide();
                    return Ok(false);
                }
                _ => return Err("Invalid motion transition".into()),
            }
            Ok(true)
        })();
        match result {
            Ok(true) => { phase += 1; phase_started = Instant::now(); phase_frame = 0; peak = 0.0; }
            Ok(false) => phase_frame += 1,
            Err(failure) => { *error.borrow_mut() = Some(failure.to_string()); let _ = ui.hide(); }
        }
        frame += 1;
    });
    Ok(())
}
