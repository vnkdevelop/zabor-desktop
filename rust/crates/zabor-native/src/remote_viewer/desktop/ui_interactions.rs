slint::include_modules!();

#[cfg(test)]
mod tests {
    use super::DesktopUiFixture;
    use slint::platform::software_renderer::MinimalSoftwareWindow;
    use slint::platform::{Key, Platform, PlatformError, WindowAdapter, WindowEvent};
    use slint::{ComponentHandle, PhysicalSize};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    struct InertPlatform(Rc<MinimalSoftwareWindow>);

    impl Platform for InertPlatform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn viewport_origin_local_escape_repeat_and_focus_loss_are_bound_to_actual_component() {
        let window = MinimalSoftwareWindow::new(Default::default());
        slint::platform::set_platform(Box::new(InertPlatform(window.clone()))).unwrap();
        window.set_size(PhysicalSize::new(900, 700));
        let ui = DesktopUiFixture::new().unwrap();
        let keys = Rc::new(RefCell::new(Vec::new()));
        let received = keys.clone();
        ui.on_key(move |text, down, modifiers| received.borrow_mut().push((text, down, modifiers)));
        let toggles = Rc::new(RefCell::new(Vec::new()));
        let received = toggles.clone();
        let weak = ui.as_weak();
        ui.on_confinement(move |value| {
            received.borrow_mut().push(value);
            weak.upgrade().unwrap().set_confined(value);
        });
        let released = Rc::new(Cell::new(0));
        let received = released.clone();
        ui.on_release(move || received.set(received.get() + 1));
        ui.show().unwrap();
        ui.set_active(true);
        slint::platform::update_timers_and_animations();
        assert!(ui.get_focused());
        assert_eq!(
            (ui.get_vx(), ui.get_vy(), ui.get_vw(), ui.get_vh()),
            (24.0, 48.0, 800.0, 600.0)
        );
        ui.set_left(30.0);
        ui.set_top(60.0);
        ui.set_video_width(640.0);
        ui.set_video_height(360.0);
        assert_eq!(
            (ui.get_vx(), ui.get_vy(), ui.get_vw(), ui.get_vh()),
            (30.0, 60.0, 640.0, 360.0)
        );
        for _ in 0..2 {
            ui.window().dispatch_event(WindowEvent::KeyPressed {
                text: Key::Escape.into(),
            });
        }
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Escape.into(),
        });
        assert_eq!(*toggles.borrow(), vec![false]);
        assert!(keys.borrow().is_empty());
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Escape.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Escape.into(),
        });
        assert_eq!(*toggles.borrow(), vec![false, true]);
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: "a".into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: "a".into() });
        assert_eq!(keys.borrow().len(), 2);
        ui.invoke_focus_away();
        slint::platform::update_timers_and_animations();
        assert!(!ui.get_focused());
        assert_eq!(released.get(), 1);
        ui.hide().unwrap();
    }
}
