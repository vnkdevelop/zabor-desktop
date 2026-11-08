use super::NativeApp;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

pub(super) fn check(ui: &NativeApp) -> Result<(), &'static str> {
    let click = |x, y| {
        let position = slint::LogicalPosition::new(x, y);
        ui.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        ui.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
    };
    let picks = Rc::new(RefCell::new(0));
    let count = picks.clone();
    ui.on_choose_avatar(move || *count.borrow_mut() += 1);
    let applied = Rc::new(RefCell::new(0));
    let count = applied.clone();
    ui.on_apply_crop(move || *count.borrow_mut() += 1);
    let weak = ui.as_weak();
    ui.on_cancel_crop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_crop_active(false);
        }
    });
    ui.set_authenticated(false);
    ui.set_auth_step(2);
    ui.set_modal(0);
    ui.set_busy(false);
    ui.set_avatar_loading(false);
    ui.invoke_choose_avatar();
    if *picks.borrow() != 1 {
        return Err("Avatar selection callback missing");
    }
    let pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        &[255u8, 0, 0, 255].repeat(32),
        8,
        4,
    );
    ui.set_crop_image(slint::Image::from_rgba8(pixels));
    ui.set_crop_scale(1.0);
    ui.set_crop_x(0.0);
    ui.set_crop_y(0.0);
    ui.set_crop_active(true);
    let position = slint::LogicalPosition::new(550.0, 346.0);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(570.0, 366.0),
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: slint::LogicalPosition::new(570.0, 366.0),
        button: PointerEventButton::Left,
    });
    if ui.get_crop_x() != 20.0 || ui.get_crop_y() != 20.0 {
        return Err("Crop dragging did not preserve preview-pixel offsets");
    }
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::RightArrow.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::RightArrow.into(),
    });
    if ui.get_crop_x() != 21.0 {
        return Err("Crop keyboard movement failed");
    }
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position,
        delta_x: 0.0,
        delta_y: 120.0,
    });
    if (ui.get_crop_scale() - 1.1).abs() > 0.001 {
        return Err("Crop wheel scaling failed");
    }
    ui.set_crop_scale(5.0);
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position,
        delta_x: 0.0,
        delta_y: 120.0,
    });
    if ui.get_crop_scale() != 5.0 {
        return Err("Crop wheel exceeded maximum scale");
    }
    ui.set_avatar_loading(true);
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position,
        delta_x: 0.0,
        delta_y: -120.0,
    });
    click(635.0, 574.0);
    if ui.get_crop_scale() != 5.0 || *applied.borrow() != 0 {
        return Err("Busy crop forwarded scale or duplicate apply");
    }
    ui.set_avatar_loading(false);
    click(403.0, 506.0);
    if ui.get_crop_scale() != 0.5 {
        return Err("Crop slider minimum/step failed");
    }
    click(635.0, 574.0);
    if *applied.borrow() != 1 {
        return Err("Crop apply pointer callback missing");
    }
    click(465.0, 574.0);
    if ui.get_crop_active() {
        return Err("Crop cancel did not close its overlay");
    }
    ui.set_authenticated(true);
    ui.set_user_id("avatar-fixture-owner".into());
    ui.set_profile_id("avatar-fixture-owner".into());
    ui.set_profile_name("".into());
    ui.set_profile_about("".into());
    ui.set_profile_editing(true);
    ui.set_profile_saving(false);
    ui.set_modal(9);
    let saves = Rc::new(RefCell::new(0));
    let count = saves.clone();
    ui.on_save_profile(move || *count.borrow_mut() += 1);
    click(550.0, 384.0);
    for _ in 0..26 {
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: "a".into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: "a".into() });
    }
    if ui.get_profile_name().len() != 20 {
        return Err("Profile display-name field did not enforce legacy maximum");
    }
    ui.set_profile_name("".into());
    click(550.0, 384.0);
    for _ in 0..11 {
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: "😀".into()
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: "😀".into()
        });
    }
    if ui.get_profile_name().encode_utf16().count() != 20
        || ui.get_profile_name().chars().count() != 10
    {
        return Err("Profile display-name field did not enforce UTF-16 maximum");
    }
    click(550.0, 476.0);
    for _ in 0..160 {
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: "b".into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: "b".into() });
    }
    if ui.get_profile_about().len() != 150 {
        return Err("Profile about-me field did not enforce legacy maximum");
    }
    ui.set_profile_about("".into());
    click(550.0, 476.0);
    for _ in 0..76 {
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: "😀".into()
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: "😀".into()
        });
    }
    if ui.get_profile_about().encode_utf16().count() != 150
        || ui.get_profile_about().chars().count() != 75
    {
        return Err("Profile about-me field did not enforce UTF-16 maximum");
    }
    ui.set_profile_saving(true);
    click(635.0, 576.0);
    if *saves.borrow() != 0 {
        return Err("Busy profile submitted a duplicate save");
    }
    ui.set_profile_saving(false);
    click(635.0, 576.0);
    if *saves.borrow() != 1 {
        return Err("Profile save pointer callback missing");
    }
    ui.set_profile_editing(false);
    ui.set_profile_id("peer-action-fixture".into());
    ui.set_profile_name("Peer".into());
    ui.set_profile_login("peer".into());
    ui.set_profile_about("".into());
    ui.set_profile_friend(true);
    ui.set_connected(true);
    ui.set_busy(false);
    ui.set_profile_online(false);
    let calls = Rc::new(RefCell::new(0));
    let count = calls.clone();
    ui.on_call_friend(move |_| *count.borrow_mut() += 1);
    click(630.0, 490.0);
    if *calls.borrow() != 0 || ui.get_modal() != 9 {
        return Err("Offline peer profile dispatched a call or closed the modal");
    }
    ui.set_profile_online(true);
    click(630.0, 490.0);
    if *calls.borrow() != 1 || ui.get_modal() != 0 {
        return Err("Online profile call pointer callback missing");
    }
    ui.set_modal(9);
    ui.set_voice_active(true);
    ui.set_active_tab(0);
    let chats = Rc::new(RefCell::new(0));
    let count = chats.clone();
    ui.on_choose_friend(move |_| *count.borrow_mut() += 1);
    click(450.0, 490.0);
    if *chats.borrow() != 1
        || ui.get_active_tab() != 1
        || !ui.get_voice_active()
        || ui.get_modal() != 0
    {
        return Err("Profile chat failed to select friends without leaving Voice");
    }
    ui.set_modal(9);
    ui.set_profile_friend(false);
    ui.set_profile_request_status(0);
    let additions = Rc::new(RefCell::new(0));
    let count = additions.clone();
    ui.on_add_profile_friend(move || *count.borrow_mut() += 1);
    click(550.0, 490.0);
    if *additions.borrow() != 1 || ui.get_modal() != 9 {
        return Err("Profile add-friend pointer callback missing or closed modal");
    }
    for status in [1, 2] {
        ui.set_profile_request_status(status);
        click(550.0, 490.0);
        if *additions.borrow() != 1 {
            return Err("Loading/sent profile add-friend dispatched a duplicate");
        }
    }
    ui.set_profile_request_status(0);
    ui.set_voice_active(false);
    ui.set_modal(0);
    ui.set_authenticated(false);
    ui.set_auth_step(0);
    Ok(())
}
