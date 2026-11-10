slint::include_modules!();

#[cfg(test)]
mod tests {
    use super::{FriendItem, NativeApp, RemoteControlPillWindow, VideoItem};
    use slint::platform::software_renderer::MinimalSoftwareWindow;
    use slint::platform::{Key, Platform, PlatformError, WindowAdapter, WindowEvent};
    use slint::{ComponentHandle, PhysicalSize, VecModel};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    struct InertPlatform(Rc<MinimalSoftwareWindow>);

    impl Platform for InertPlatform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
            Ok(self.0.clone())
        }
    }

    fn render(window: &MinimalSoftwareWindow) {
        let size = window.window().size();
        let mut pixels =
            vec![slint::Rgb8Pixel::default(); size.width as usize * size.height as usize];
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, size.width as usize);
        });
        slint::platform::update_timers_and_animations();
    }

    #[test]
    fn actual_app_forwards_client_viewport_and_confined_escape_without_remote_keys_or_native_window(
    ) {
        let window = MinimalSoftwareWindow::new(Default::default());
        slint::platform::set_platform(Box::new(InertPlatform(window.clone()))).unwrap();
        window.set_size(PhysicalSize::new(1200, 900));
        let ui = NativeApp::new().unwrap();
        let _export: fn() -> Result<RemoteControlPillWindow, slint::PlatformError> =
            RemoteControlPillWindow::new;
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let received = bounds.clone();
        ui.on_control_viewport(move |peer, x, y, width, height| {
            received
                .borrow_mut()
                .push((peer.to_string(), [x, y, width, height]));
        });
        let toggles = Rc::new(RefCell::new(Vec::new()));
        let received = toggles.clone();
        let weak = ui.as_weak();
        ui.on_control_confinement(move |peer, confined| {
            assert_eq!(peer, "peer");
            received.borrow_mut().push(confined);
            weak.upgrade().unwrap().set_control_confined(confined);
        });
        let keys = Rc::new(RefCell::new(Vec::new()));
        let received = keys.clone();
        ui.on_control_key(move |peer, text, down, modifiers| {
            received.borrow_mut().push((peer, text, down, modifiers));
        });
        let releases = Rc::new(Cell::new(0));
        let received = releases.clone();
        let weak = ui.as_weak();
        ui.on_release_control(move || {
            received.set(received.get() + 1);
            weak.upgrade().unwrap().set_control_ready(false);
        });
        ui.on_control_can_request(|_| false);
        ui.on_tr(|key, _| key);
        ui.on_limit_text(|value, _| value);
        ui.set_authenticated(true);
        ui.set_voice_users(
            Rc::new(VecModel::from(vec![FriendItem {
                id: "peer".into(),
                name: "Controller".into(),
                ..Default::default()
            }]))
            .into(),
        );
        ui.set_video_frames(
            Rc::new(VecModel::from(vec![VideoItem {
                id: "peer".into(),
                title: "Controller".into(),
                frame: slint::Image::from_rgba8(
                    slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(160, 96),
                ),
            }]))
            .into(),
        );
        ui.set_voice_active(true);
        ui.set_focused_video("peer".into());
        ui.set_control_viewer("peer".into());
        ui.set_control_confined(true);
        ui.show().unwrap();
        render(&window);
        ui.set_control_ready(true);
        slint::platform::update_timers_and_animations();
        render(&window);
        let original = bounds.borrow().last().cloned().unwrap();
        assert_eq!(original.0, "peer");
        assert!(original.1[0] >= 356.0 && original.1[1] >= 132.0);
        assert!(original.1[2] > 0.0 && original.1[3] > 0.0);
        for _ in 0..2 {
            ui.window().dispatch_event(WindowEvent::KeyPressed {
                text: Key::Escape.into(),
            });
        }
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Escape.into(),
        });
        assert_eq!(*toggles.borrow(), vec![false]);
        assert!(!ui.get_control_confined());
        assert!(keys.borrow().is_empty());
        window.set_size(PhysicalSize::new(1400, 1000));
        slint::platform::update_timers_and_animations();
        render(&window);
        let resized = bounds.borrow().last().cloned().unwrap();
        assert_ne!(original.1, resized.1);
        ui.set_modal(3);
        slint::platform::update_timers_and_animations();
        assert!(releases.get() >= 1);
        assert!(!ui.get_control_ready());
        ui.hide().unwrap();
    }
}
