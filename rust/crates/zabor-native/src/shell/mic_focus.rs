use crate::{MicTestView, NativeApp};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub(super) fn key(ui: &NativeApp, key: Key, down_only: bool) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
    if !down_only {
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
    }
}

pub(super) fn pointer(ui: &NativeApp, control: &str, pressed: bool) {
    let origin_x = (ui.window().size().width as f32 / ui.window().scale_factor() - 156.0) / 2.0;
    let (x, y) = match control {
        "seek" => (origin_x + 96.0, ui.get_mic_test_panel_y() + 18.0),
        "playback" => (origin_x + 19.0, ui.get_mic_test_panel_y() + 18.0),
        "primary" => (origin_x + 78.0, ui.get_mic_test_panel_y() + 18.0),
        "retake" => (origin_x + 78.0, ui.get_mic_test_panel_y() + 52.5),
        _ => (
            ui.window().size().width as f32 / ui.window().scale_factor() - 8.0,
            8.0,
        ),
    };
    let position = slint::LogicalPosition::new(x, y);
    ui.window().dispatch_event(if pressed {
        WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        }
    } else {
        WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        }
    });
}

pub(super) fn save_panel(
    ui: &NativeApp,
    path: &PathBuf,
    panel_y: f32,
    panel_height: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let pixels = ui.window().take_snapshot()?;
    let scale = ui.window().scale_factor();
    let x = ((pixels.width() as f32 - 156.0 * scale) / 2.0 - 6.0 * scale).round() as usize;
    let y = ((panel_y - 6.0) * scale).round() as usize;
    let width = (168.0 * scale).round() as usize;
    let height = ((panel_height + 12.0) * scale).ceil() as usize;
    if x + width > pixels.width() as usize || y + height > pixels.height() as usize {
        return Err("Focus evidence crop leaves the window".into());
    }
    let stride = pixels.width() as usize * 4;
    let mut crop = vec![0_u8; width * height * 4];
    for row in 0..height {
        crop[row * width * 4..(row + 1) * width * 4].copy_from_slice(
            &pixels.as_bytes()[(y + row) * stride + x * 4..(y + row) * stride + (x + width) * 4],
        );
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
) -> Result<(), Box<dyn std::error::Error>> {
    if output.exists() {
        return Err("Focus evidence directory already exists".into());
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
    ui.on_mic_test_toggle(move || {
        if let Some(ui) = weak.upgrade() {
            let mut test = ui.get_mic_test();
            test.playing = !test.playing;
            ui.set_mic_test(test);
        }
    });
    let states = ["idle", "arming", "recording", "paused", "silent", "failed"];
    let mut steps = Vec::new();
    for state in states {
        steps.push((state, "initial"));
        if matches!(state, "arming" | "recording") {
            steps.push((state, "disabled-tab"));
        } else {
            steps.push((state, "tab-first"));
            if matches!(state, "paused" | "silent") {
                for action in [
                    "tab-seek",
                    "tab-retake",
                    "reverse-seek",
                    "pointer-seek-down",
                    "pointer-seek-up",
                    "pointer-to-keyboard",
                    "keyboard-modifier",
                    "pointer-playback-down",
                    "pointer-playback-up",
                    "pointer-retake-down",
                    "pointer-retake-cancel",
                    "pointer-modifier",
                    "reverse-from-pointer",
                    "reverse-playback",
                    "space-unarmed-up",
                    "space-down",
                    "space-up",
                    "space-repeat-up",
                    "space-arm-transfer",
                    "space-transfer-up",
                    "space-return-playback",
                    "space-after-transfer-up",
                    "hidden",
                    "hidden-tab",
                ] {
                    steps.push((state, action));
                }
            } else {
                for action in [
                    "pointer-primary-down",
                    "pointer-primary-cancel",
                    "pointer-modifier",
                    "pointer-to-keyboard",
                ] {
                    steps.push((state, action));
                }
            }
            for action in [
                "fresh-initial",
                "fresh-pointer-down",
                "fresh-pointer-cancel",
                "fresh-pointer-control",
                "fresh-pointer-alt",
                "fresh-pointer-shift",
                "fresh-keyboard-pointer-down",
                "fresh-keyboard-pointer-cancel",
            ] {
                steps.push((state, action));
            }
        }
    }
    let weak = ui.as_weak();
    let mut index = 0;
    let mut pending = false;
    let started = Instant::now();
    let mut last_action = started;
    let mut panel_y = ui.get_mic_test_panel_y();
    timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        let Some(ui) = weak.upgrade() else { return; };
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            if started.elapsed() > Duration::from_secs(90) {
                return Err("Focus evidence exceeded its deadline".into());
            }
            if last_action.elapsed() < Duration::from_millis(if index == 0 && !pending { 500 } else { 64 }) {
                return Ok(());
            }
            let Some(&(state, action)) = steps.get(index) else {
                println!("{}", serde_json::json!({"nativeMicFocusCheck":"completed","captures":index,"physicalDevices":false}));
                ui.hide()?;
                return Ok(());
            };
            if pending {
                let file = format!("native-focus-{state}-{action}-{}-{}.png", ui.get_language(), ui.window().scale_factor());
                let panel_height = 36.0+if matches!(state,"paused"|"silent"){27.0}else{0.0}+if matches!(state,"failed"|"silent"){19.75}else{0.0};
                if ui.get_mic_test_active() { panel_y = ui.get_mic_test_panel_y(); }
                save_panel(&ui, &output.join(&file), panel_y, panel_height)?;
                println!("{}", serde_json::json!({"nativeMicFocusCapture":{
                    "state":state,"action":action,"file":file,"scale":ui.window().scale_factor(),
                    "textScale":ui.get_mic_test_text_scale(),"focusedControl":ui.get_mic_test_focused_control().as_str(),
                    "focusVisible":ui.get_mic_test_focus_outline(),"hidden":!ui.get_mic_test_active(),
                    "playing":ui.get_mic_test().playing,"position":ui.get_mic_test().position,
                    "physicalDevices":false,"panelX":(ui.window().size().width as f32 / ui.window().scale_factor()-156.0)/2.0,
                    "panelY":panel_y,"panelWidth":156,"panelHeight":panel_height
                }}));
                index += 1;
                pending = false;
            } else {
                match action {
                    "initial" | "fresh-initial" => {
                        ui.set_settings_tab(1);
                        ui.set_mic_test_revealed(true);
                        ui.set_mic_test(MicTestView {
                            stage: match state { "paused"|"silent"=>"ready", "failed"=>"idle", other=>other }.into(),
                            issue: match state { "failed"=>"failed", "silent"=>"silent", _=>"" }.into(),
                            playing: state == "silent", duration: if matches!(state,"paused"|"silent") {5.0} else {0.0}, position:1.5,
                        });
                        ui.set_mic_test_focus_reset(ui.get_mic_test_focus_reset() + 1);
                    }
                    "tab-first" => {
                        for _ in 0..128 {
                            key(&ui, Key::Tab, false);
                            if !ui.get_mic_test_focused_control().is_empty() { break; }
                        }
                    }
                    "disabled-tab" | "hidden-tab" => { for _ in 0..16 { key(&ui, Key::Tab, false); } }
                    "tab-seek" | "tab-retake" => key(&ui, Key::Tab, false),
                    "reverse-seek" | "reverse-from-pointer" | "reverse-playback" => {
                        key(&ui, Key::Shift, true);
                        key(&ui, Key::Tab, false);
                        ui.window().dispatch_event(WindowEvent::KeyReleased { text: Key::Shift.into() });
                    }
                    "pointer-seek-down" => pointer(&ui, "seek", true),
                    "pointer-seek-up" => pointer(&ui, "seek", false),
                    "pointer-to-keyboard" => key(&ui, Key::RightArrow, false),
                    "keyboard-modifier" | "pointer-modifier" => key(&ui, Key::Shift, false),
                    "fresh-pointer-down" | "fresh-keyboard-pointer-down" => pointer(&ui, if matches!(state,"paused"|"silent") { "playback" } else { "primary" }, true),
                    "fresh-pointer-cancel" | "fresh-keyboard-pointer-cancel" => pointer(&ui, "", false),
                    "fresh-pointer-control" => key(&ui, Key::Control, false),
                    "fresh-pointer-alt" => key(&ui, Key::Alt, false),
                    "fresh-pointer-shift" => key(&ui, Key::Shift, false),
                    "pointer-playback-down" => pointer(&ui, "playback", true),
                    "pointer-playback-up" => pointer(&ui, "playback", false),
                    "pointer-retake-down" => pointer(&ui, "retake", true),
                    "pointer-primary-down" => pointer(&ui, "primary", true),
                    "pointer-retake-cancel" | "pointer-primary-cancel" => pointer(&ui, "", false),
                    "space-down" => key(&ui, Key::Space, true),
                    "space-up" | "space-unarmed-up" | "space-repeat-up" | "space-transfer-up" | "space-after-transfer-up" => ui.window().dispatch_event(WindowEvent::KeyReleased { text: Key::Space.into() }),
                    "space-arm-transfer" => { key(&ui, Key::Space, true); key(&ui, Key::Tab, false); }
                    "space-return-playback" => { key(&ui, Key::Shift, true); key(&ui, Key::Tab, false); ui.window().dispatch_event(WindowEvent::KeyReleased { text: Key::Shift.into() }); }
                    "hidden" => { ui.set_settings_tab(0); let mut test=ui.get_mic_test(); test.playing=false; ui.set_mic_test(test); }
                    _ => return Err("Unsupported focus evidence action".into()),
                }
                pending = true;
            }
            last_action = Instant::now();
            Ok(())
        })();
        if let Err(failure) = result {
            *error.borrow_mut() = Some(failure.to_string());
            let _ = ui.hide();
        }
    });
    Ok(())
}
