use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Key, Platform, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, PhysicalSize};
use std::rc::Rc;

slint::slint! {
    import { FormButton, IconButton, ControlMotion } from "../ui/controls.slint";
    export component ControlsFixture inherits Window {
        width: 240px;
        height: 120px;
        in-out property <bool> disabled;
        in-out property <bool> reduced-motion;
        in-out property <int> form-count;
        in-out property <int> icon-count;
        out property <float> form-scale: form.transform-scale-x;
        out property <float> icon-scale: icon.transform-scale-x;
        changed reduced-motion => { ControlMotion.reduced-motion = root.reduced-motion; }
        public function focus-form() { form.focus(); }
        public function focus-icon() { icon.focus(); }
        form := FormButton { x: 8px; y: 8px; width: 160px; label: "fixture"; disabled: root.disabled; activated => { root.form-count += 1; } }
        icon := IconButton { x: 184px; y: 8px; disabled: root.disabled; activated => { root.icon-count += 1; } }
    }
}

struct Headless;

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        window.set_size(PhysicalSize::new(240, 120));
        Ok(window)
    }
}

fn key(ui: &ControlsFixture, key: Key, down: bool) {
    let text = key.into();
    ui.window().dispatch_event(if down {
        WindowEvent::KeyPressed { text }
    } else {
        WindowEvent::KeyReleased { text }
    });
}

#[test]
fn buttons_reject_unarmed_releases_repeat_and_transferred_focus() {
    slint::platform::set_platform(Box::new(Headless)).unwrap();
    let ui = ControlsFixture::new().unwrap();
    ui.show().unwrap();
    ui.invoke_focus_form();
    key(&ui, Key::Space, false);
    assert_eq!(ui.get_form_count(), 0);
    key(&ui, Key::Space, true);
    key(&ui, Key::Space, true);
    assert_eq!(ui.get_form_count(), 0);
    key(&ui, Key::Space, false);
    assert_eq!(ui.get_form_count(), 1);
    key(&ui, Key::Space, false);
    assert_eq!(ui.get_form_count(), 1);
    key(&ui, Key::Return, true);
    key(&ui, Key::Return, true);
    assert_eq!(ui.get_form_count(), 2);
    key(&ui, Key::Return, false);
    key(&ui, Key::Return, true);
    assert_eq!(ui.get_form_count(), 3);
    key(&ui, Key::Return, false);
    key(&ui, Key::Space, true);
    ui.invoke_focus_icon();
    key(&ui, Key::Space, false);
    assert_eq!(ui.get_form_count(), 3);
    assert_eq!(ui.get_icon_count(), 0);
    key(&ui, Key::Space, true);
    key(&ui, Key::Space, false);
    assert_eq!(ui.get_icon_count(), 1);
    key(&ui, Key::Return, true);
    key(&ui, Key::Return, true);
    assert_eq!(ui.get_icon_count(), 2);
    key(&ui, Key::Return, false);
    key(&ui, Key::Space, true);
    ui.set_disabled(true);
    key(&ui, Key::Space, false);
    assert_eq!(ui.get_icon_count(), 2);
    ui.set_disabled(false);
    ui.invoke_focus_icon();
    key(&ui, Key::Space, false);
    assert_eq!(ui.get_icon_count(), 2);
    ui.set_reduced_motion(true);
    key(&ui, Key::Space, true);
    assert_eq!(ui.get_icon_scale(), 1.0);
    key(&ui, Key::Space, false);
    assert_eq!(ui.get_icon_count(), 3);
    ui.invoke_focus_form();
    key(&ui, Key::Space, true);
    assert_eq!(ui.get_form_scale(), 1.0);
    key(&ui, Key::Space, false);
    assert_eq!(ui.get_form_count(), 4);
    ui.hide().unwrap();
}
