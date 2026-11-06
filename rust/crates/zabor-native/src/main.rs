use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};
use zabor_native::localization::Catalog;
use zabor_native::motion::updater_phase;
use zabor_native::release::TARGET_BETA_VERSION;
use zabor_native::update::{Effect, Phase, UpdateModel};

slint::include_modules!();
mod shell;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.is_empty()
        || arguments.iter().any(|argument| argument == "--app")
        || arguments
            .windows(2)
            .any(|pair| pair[0] == "--preview" && pair[1] != "updater")
    {
        return shell::run(&arguments);
    }
    if arguments.iter().any(|argument| argument == "--help") {
        println!("Native client under development, not a released client.\n--app --server URL --locale ru|en\n--preview login|confirm|setup|home --snapshot FILE.png --scale 1|1.25|1.5|2\n--preview updater --state downloading|installing|error --locale ru|en\n--snapshot FILE.png --at-ms 0 --scale 1|1.25|1.5|2 --reduced-motion\n--interaction-check");
        return Ok(());
    }
    let mut locale = "ru".to_string();
    let mut phase = Phase::Downloading;
    let mut snapshot: Option<PathBuf> = None;
    let mut preview = false;
    let mut interaction_check = false;
    let mut reduced_motion = false;
    let mut at_ms = 0_u64;
    let mut scale = 1.0_f32;
    let mut index = 0;
    while index < arguments.len() {
        let key = &arguments[index];
        if key == "--interaction-check" {
            interaction_check = true;
            index += 1;
            continue;
        }
        if key == "--reduced-motion" {
            reduced_motion = true;
            index += 1;
            continue;
        }
        let value = arguments.get(index + 1).ok_or("Missing command argument")?;
        match key.as_str() {
            "--preview" if value == "updater" => preview = true,
            "--locale" => locale = value.clone(),
            "--state" => {
                phase = match value.as_str() {
                    "downloading" => Phase::Downloading,
                    "installing" => Phase::Installing,
                    "error" => Phase::Error,
                    _ => return Err("Unsupported preview state".into()),
                }
            }
            "--snapshot" => snapshot = Some(PathBuf::from(value)),
            "--at-ms" => at_ms = value.parse()?,
            "--scale" => scale = value.parse()?,
            _ => return Err(format!("Unsupported option: {key}").into()),
        }
        index += 2;
    }
    if !preview || ![1.0, 1.25, 1.5, 2.0].contains(&scale) {
        return Err("A supported preview and scale are required".into());
    }
    if interaction_check && snapshot.is_some() {
        return Err("Choose either an interaction check or a snapshot".into());
    }
    if interaction_check {
        phase = Phase::Error;
    }
    if snapshot.as_ref().is_some_and(|path| path.exists()) {
        return Err("Snapshot already exists; choose another output path".into());
    }
    if arguments.iter().any(|argument| argument == "--scale") {
        std::env::set_var("SLINT_SCALE_FACTOR", scale.to_string());
    }
    let catalog = Rc::new(Catalog::load(&locale)?);
    let model = Rc::new(RefCell::new(UpdateModel::new(
        phase,
        44_040_192,
        104_857_600,
    )));
    let ui = NativeUpdater::new()?;
    ui.set_wordmark(catalog.get("title").into());
    ui.set_close_label(catalog.get("close").into());
    ui.set_version_label(format!("{} {TARGET_BETA_VERSION}", catalog.get("version")).into());
    ui.set_reduced_motion(reduced_motion);
    apply_model(&ui, &model.borrow(), &catalog);
    ui.set_motion_phase(updater_phase(
        at_ms,
        phase == Phase::Installing,
        reduced_motion,
    ));
    ui.set_spinner_phase(if reduced_motion {
        0.0
    } else {
        (at_ms % 1000) as f32 / 1000.0
    });
    let weak = ui.as_weak();
    let close_model = model.clone();
    ui.on_close_requested(move || {
        let effect = close_model.borrow_mut().close();
        println!("preview event: {effect:?}");
        if let Some(ui) = weak.upgrade() {
            let _ = ui.hide();
        }
    });
    let window_close_model = model.clone();
    ui.window().on_close_requested(move || {
        println!(
            "preview event: {:?}",
            window_close_model.borrow_mut().close()
        );
        slint::CloseRequestResponse::HideWindow
    });
    let action_model = model.clone();
    let action_catalog = catalog.clone();
    let weak = ui.as_weak();
    ui.on_action_requested(move || {
        let effect = action_model.borrow_mut().action();
        println!("preview event: {effect:?}; no network or installer operation executed");
        if let Some(ui) = weak.upgrade() {
            if matches!(effect, Effect::CancelDownload(_) | Effect::CloseWindow) {
                let _ = ui.hide();
            } else {
                apply_model(&ui, &action_model.borrow(), &action_catalog);
            }
        }
    });
    let weak = ui.as_weak();
    ui.on_drag_requested(move || {
        if let Some(ui) = weak.upgrade() {
            ui.window().with_winit_window(|window| {
                let _ = window.drag_window();
            });
        }
    });
    let native_timer = Timer::default();
    let snapshot_error = Rc::new(RefCell::new(None::<String>));
    if interaction_check {
        let weak = ui.as_weak();
        let error = snapshot_error.clone();
        native_timer.start(
            TimerMode::SingleShot,
            Duration::from_millis(250),
            move || {
                if let Some(ui) = weak.upgrade() {
                    if let Err(failure) = check_interactions(&ui, &model, &catalog) {
                        *error.borrow_mut() = Some(failure.to_string());
                    }
                    let _ = ui.hide();
                }
            },
        );
    } else if let Some(path) = snapshot {
        let weak = ui.as_weak();
        let error = snapshot_error.clone();
        native_timer.start(
            TimerMode::SingleShot,
            Duration::from_millis(250),
            move || {
                if let Some(ui) = weak.upgrade() {
                    match save_snapshot(&ui, &path) {
                        Ok(()) => {
                            println!("{}", serde_json::json!({"snapshot": path, "native": true}))
                        }
                        Err(failure) => *error.borrow_mut() = Some(failure.to_string()),
                    }
                    let _ = ui.hide();
                }
            },
        );
    } else if !reduced_motion {
        let start = Instant::now();
        let weak = ui.as_weak();
        native_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            if let Some(ui) = weak.upgrade() {
                if model.borrow().phase != Phase::Error {
                    ui.set_motion_phase(updater_phase(
                        (start.elapsed().as_millis() as u64).saturating_add(at_ms),
                        model.borrow().phase == Phase::Installing,
                        false,
                    ));
                    ui.set_spinner_phase(
                        ((start.elapsed().as_millis() + u128::from(at_ms)) % 1000) as f32 / 1000.0,
                    );
                }
            }
        });
    }
    ui.run()?;
    if let Some(error) = snapshot_error.borrow_mut().take() {
        return Err(error.into());
    }
    Ok(())
}

fn apply_model(ui: &NativeUpdater, model: &UpdateModel, catalog: &Catalog) {
    let (phase, heading, action, detail) = match model.phase {
        Phase::Downloading => (0, "downloading", "cancel", ""),
        Phase::Installing => (1, "installing", "close", "installingDesc"),
        Phase::Error => (2, "errorTitle", "retry", "betaUnverified"),
        Phase::Closed => return,
    };
    ui.set_phase(phase);
    ui.set_focused_control(0);
    ui.set_heading(catalog.get(heading).into());
    ui.set_action_label(catalog.get(action).into());
    ui.set_detail(catalog.get(detail).into());
    ui.set_footer(catalog.get("pleaseWait").into());
    ui.set_percent(i32::from(model.percent()));
    ui.set_transferred_label(
        if model.total == 0 {
            catalog.megabytes(model.transferred)
        } else {
            format!(
                "{} / {}",
                catalog.megabytes(model.transferred),
                catalog.megabytes(model.total)
            )
        }
        .into(),
    );
}

fn save_snapshot(ui: &NativeUpdater, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let pixels = ui.window().take_snapshot()?;
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), pixels.width(), pixels.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(pixels.as_bytes())?;
    writer.finish()?;
    Ok(())
}

fn check_interactions(
    ui: &NativeUpdater,
    model: &Rc<RefCell<UpdateModel>>,
    catalog: &Catalog,
) -> Result<(), &'static str> {
    use slint::platform::{Key, WindowEvent};
    for expected in [1, 2, 3] {
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Tab.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Tab.into(),
        });
        if ui.get_focused_control() != expected {
            return Err("Native keyboard focus order mismatch");
        }
    }
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Return.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Return.into(),
    });
    if model.borrow().phase != Phase::Downloading || model.borrow().operation != 2 {
        return Err("Keyboard retry did not enter a new download operation");
    }
    if ui.get_focused_control() != 0 {
        return Err("Retry left focus on a removed error action");
    }
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Return.into(),
    });
    if model.borrow().phase != Phase::Downloading {
        return Err("An unfocused key activated a removed control");
    }
    for (phase, count) in [
        (Phase::Downloading, 2),
        (Phase::Installing, 1),
        (Phase::Error, 3),
    ] {
        *model.borrow_mut() = UpdateModel::new(phase, 42, 100);
        apply_model(ui, &model.borrow(), catalog);
        for expected in (1..=count).chain(std::iter::once(1)) {
            ui.window().dispatch_event(WindowEvent::KeyPressed {
                text: Key::Tab.into(),
            });
            ui.window().dispatch_event(WindowEvent::KeyReleased {
                text: Key::Tab.into(),
            });
            if ui.get_focused_control() != expected {
                return Err("Focus traversal included an unavailable action");
            }
        }
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Shift.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Tab.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Tab.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Shift.into(),
        });
        if ui.get_focused_control() != count {
            return Err("Reverse keyboard traversal did not wrap correctly");
        }
    }
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Space.into(),
    });
    if model.borrow().phase != Phase::Error {
        return Err("Space activated retry before key release");
    }
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Space.into(),
    });
    if model.borrow().phase != Phase::Downloading || model.borrow().operation != 2 {
        return Err("Space release did not activate retry");
    }
    let position = slint::LogicalPosition::new(399.0, 18.0);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    if model.borrow().phase != Phase::Closed {
        return Err("Native close button did not cancel the preview download");
    }
    println!(
        "{}",
        serde_json::json!({"nativeInteractionCheck":"passed","keyboardRetry":true,"pointerClose":true,"phaseFocusOrder":true,"reverseTab":true,"retryFocusReset":true,"spaceRelease":true})
    );
    Ok(())
}
