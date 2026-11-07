use super::mic_focus::{key, pointer, save_panel};
use crate::{MicTestView, NativeApp};
use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

fn tab(ui: &NativeApp, reverse: bool) {
    if reverse {
        key(ui, Key::Shift, true);
    }
    key(ui, Key::Tab, false);
    if reverse {
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Shift.into(),
        });
    }
}

pub(super) fn start(
    ui: &NativeApp,
    timer: &Timer,
    output: PathBuf,
    error: Rc<RefCell<Option<String>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    if output.exists() {
        return Err("Pressed evidence directory already exists".into());
    }
    std::fs::create_dir_all(&output)?;
    let calls = Rc::new(Cell::new(0_u32));
    let record_calls = calls.clone();
    let weak = ui.as_weak();
    ui.on_mic_test_record(move || {
        if let Some(ui) = weak.upgrade() {
            let test = ui.get_mic_test();
            if ui.get_mic_test_active() && matches!(test.stage.as_str(), "idle" | "ready") {
                record_calls.set(record_calls.get() + 1);
                ui.set_mic_test(MicTestView {
                    stage: "arming".into(),
                    ..MicTestView::default()
                });
            }
        }
    });
    let scenarios: &[(&str, &[&str])] = &[
        (
            "space",
            &[
                "rest",
                "unarmed-up",
                "space-held",
                "repeat-down",
                "space-up",
                "repeat-up",
            ],
        ),
        (
            "transfer",
            &[
                "rest",
                "space-held",
                "transfer-away",
                "transfer-return",
                "space-up",
            ],
        ),
        (
            "blur",
            &[
                "rest",
                "space-held",
                "blur",
                "refocus-down",
                "refocus-cancel",
                "space-up",
            ],
        ),
        ("disable", &["rest", "space-held", "disable", "space-up"]),
        (
            "pointer",
            &[
                "pointer-rest",
                "pointer-down",
                "pointer-outside",
                "pointer-return",
                "pointer-cancel",
            ],
        ),
        (
            "pointer-commit",
            &[
                "pointer-rest",
                "pointer-down",
                "pointer-outside",
                "pointer-return",
                "pointer-up",
            ],
        ),
        (
            "keyboard-pointer",
            &["rest", "pointer-down", "pointer-cancel"],
        ),
        ("rapid", &["rest", "rapid-transfer", "rapid-click"]),
    ];
    let steps: Vec<_> = ["idle", "failed", "paused", "silent"]
        .into_iter()
        .flat_map(|state| {
            scenarios.iter().flat_map(move |(scenario, actions)| {
                actions
                    .iter()
                    .map(move |action| (state, *scenario, *action))
            })
        })
        .collect();
    let weak = ui.as_weak();
    let started = Instant::now();
    let mut last = started;
    let mut index = 0;
    let mut phase = 0;
    let mut panel_y = ui.get_mic_test_panel_y();
    timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        let Some(ui) = weak.upgrade() else { return; };
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            if started.elapsed() > Duration::from_secs(90) {
                return Err("Pressed evidence exceeded its deadline".into());
            }
            if last.elapsed() < Duration::from_millis(if index == 0 && phase == 0 { 500 } else { 64 }) {
                return Ok(());
            }
            let Some(&(state, scenario, action)) = steps.get(index) else {
                println!("{}", serde_json::json!({"nativeMicPressedCheck":"completed","captures":index,"physicalDevices":false}));
                ui.hide()?;
                return Ok(());
            };
            let ready = matches!(state, "paused" | "silent");
            let target = if ready { "retake" } else { "primary" };
            if phase == 0 && matches!(action, "rest" | "pointer-rest") {
                ui.set_modal(3);
                ui.set_settings_tab(1);
                ui.set_mic_test_revealed(true);
                ui.set_mic_test(MicTestView {
                    stage: if ready { "ready" } else { "idle" }.into(),
                    issue: match state { "failed" => "failed", "silent" => "silent", _ => "" }.into(),
                    playing: state == "silent", duration: if ready { 5.0 } else { 0.0 }, position: 1.5,
                });
                ui.set_mic_test_focus_reset(ui.get_mic_test_focus_reset() + 1);
                calls.set(0);
                phase = 1;
            } else if phase < 2 {
                match action {
                    "rest" => {
                        for _ in 0..128 {
                            tab(&ui, false);
                            if ui.get_mic_test_focused_control() == target { break; }
                        }
                    }
                    "pointer-rest" => {},
                    "space-held" | "repeat-down" => key(&ui, Key::Space, true),
                    "unarmed-up" | "space-up" | "repeat-up" => ui.window().dispatch_event(WindowEvent::KeyReleased { text: Key::Space.into() }),
                    "transfer-away" => tab(&ui, false),
                    "transfer-return" => tab(&ui, true),
                    "blur" => ui.set_mic_test_focus_reset(ui.get_mic_test_focus_reset() + 1),
                    "refocus-down" | "pointer-down" => pointer(&ui, target, true),
                    "refocus-cancel" | "pointer-cancel" => pointer(&ui, "", false),
                    "pointer-up" => pointer(&ui, target, false),
                    "pointer-outside" | "pointer-return" => {
                        let x = if action == "pointer-return" { ui.window().size().width as f32 / ui.window().scale_factor() / 2.0 } else { 8.0 };
                        let y = if action == "pointer-return" { ui.get_mic_test_panel_y() + if ready { 52.5 } else { 18.0 } } else { 8.0 };
                        ui.window().dispatch_event(WindowEvent::PointerMoved { position: slint::LogicalPosition::new(x, y) });
                    }
                    "disable" => { ui.invoke_mic_test_record(); },
                    "rapid-transfer" => { key(&ui, Key::Space, true); tab(&ui, false); tab(&ui, true); ui.window().dispatch_event(WindowEvent::KeyReleased { text: Key::Space.into() }); },
                    "rapid-click" => key(&ui, Key::Space, false),
                    _ => return Err("Unsupported pressed evidence action".into()),
                }
                phase = 2;
            } else {
                let test = ui.get_mic_test();
                let connected = if ready { test.stage == "ready" } else { test.stage != "ready" };
                let feedback = if ready { ui.get_mic_test_retake_feedback() } else { ui.get_mic_test_primary_feedback() };
                let panel_height = 36.0 + if ready { 27.0 } else { 0.0 } + if matches!(state, "failed" | "silent") { 19.75 } else { 0.0 };
                if calls.get() == 0 { panel_y = ui.get_mic_test_panel_y(); }
                let file = format!("native-pressed-{state}-{scenario}-{action}-{}-{}.png", ui.get_language(), ui.window().scale_factor());
                save_panel(&ui, &output.join(&file), panel_y, panel_height)?;
                let focused_control = ui.get_mic_test_focused_control();
                println!("{}", serde_json::json!({"nativeMicPressedCapture":{
                    "state":state,"scenario":scenario,"action":action,"file":file,"scale":ui.window().scale_factor(),"textScale":ui.get_mic_test_text_scale(),
                    "target":target,"connected":connected,"disabled":feedback.disabled,"pointerPressed":feedback.pointer_pressed,"spaceHeld":feedback.space_held,
                    "scaleX":feedback.scale,"scaleY":feedback.scale,"outlineScale":feedback.outline_scale,"focusedControl":if connected {focused_control.as_str()} else {""},"focusVisible":connected && ui.get_mic_test_focus_outline(),
                    "recordCalls":calls.get(),"primaryDisabled":test.stage == "arming","panelActive":ui.get_mic_test_active(),"physicalDevices":false,
                    "panelX":(ui.window().size().width as f32 / ui.window().scale_factor()-156.0)/2.0,"panelY":panel_y,"panelWidth":156,"panelHeight":panel_height
                }}));
                phase = 0;
                index += 1;
            }
            last = Instant::now();
            Ok(())
        })();
        if let Err(failure) = result {
            *error.borrow_mut() = Some(failure.to_string());
            let _ = ui.hide();
        }
    });
    Ok(())
}
