use super::*;

#[derive(Default)]
struct Fake {
    calls: Vec<Packet>,
    foreground: bool,
    physical: u8,
    fail_release: bool,
    finished: bool,
}

impl Backend for Fake {
    fn activity(&mut self) -> Result<(bool, bool), InputError> {
        Ok((false, false))
    }
    fn foreground(&self, _: &CaptureLease) -> bool {
        self.foreground
    }
    fn cursor(&self) -> Result<(i32, i32), InputError> {
        Ok((-100, 200))
    }
    fn move_to(&mut self, _: (i32, i32)) -> Result<(), InputError> {
        self.calls.push(Packet::Move { x: 0.0, y: 0.0 });
        Ok(())
    }
    fn mouse(&mut self, button: u8, down: bool) -> Result<(), InputError> {
        self.calls.push(Packet::Mouse { button, down });
        if !down && self.fail_release {
            Err(InputError::Unavailable)
        } else {
            Ok(())
        }
    }
    fn wheel(&mut self, vertical: i16, horizontal: i16) -> Result<(), InputError> {
        self.calls.push(Packet::Wheel {
            vertical,
            horizontal,
        });
        Ok(())
    }
    fn key(&mut self, vk: u16, down: bool) -> Result<(), InputError> {
        self.calls.push(Packet::Key {
            vk,
            down,
            modifiers: 0,
        });
        if !down && self.fail_release {
            Err(InputError::Unavailable)
        } else {
            Ok(())
        }
    }
    fn physical_modifiers(&self) -> u8 {
        self.physical
    }
    fn host_cursor(&mut self, _: bool) -> Result<(), InputError> {
        Ok(())
    }
    fn finish(&mut self) -> Result<(), InputError> {
        self.finished = true;
        Ok(())
    }
}

fn owner() -> Owner<Fake> {
    Owner {
        backend: Fake {
            foreground: true,
            ..Default::default()
        },
        keys: BTreeSet::new(),
        buttons: BTreeSet::new(),
        cleanup_failed: Arc::new(AtomicBool::new(false)),
        finished: false,
    }
}

fn geometry() -> (Arc<CaptureLease>, Rect) {
    let lease = CaptureLease::new(Some(123));
    let rect = Rect {
        left: -1920,
        top: 120,
        width: 1920,
        height: 1080,
    };
    lease.update(rect);
    (lease, rect)
}

#[test]
fn negative_origins_and_edges_map_to_actual_capture_and_virtual_desktop() {
    let (_, rect) = geometry();
    assert_eq!(rect.point(0.0, 0.0), Some((-1920, 120)));
    assert_eq!(rect.point(1.0, 1.0), Some((-1, 1199)));
    let desktop = Rect {
        left: -1920,
        top: 0,
        width: 3840,
        height: 1200,
    };
    assert_eq!(desktop.absolute((-1920, 0)), Some((0, 0)));
    assert_eq!(desktop.absolute((1919, 1199)), Some((65535, 65535)));
    assert!(desktop.absolute((1920, 1199)).is_none());
    assert!(Rect {
        left: i32::MAX,
        ..rect
    }
    .point(0.0, 0.0)
    .is_none());
}

#[test]
fn capture_retirement_never_reopens_from_a_late_frame_or_invalid_rect() {
    let (lease, rect) = geometry();
    assert_eq!(lease.snapshot(), Some(rect));
    lease.revoke();
    lease.update(rect);
    assert!(lease.snapshot().is_none());
    let lease = CaptureLease::new(None);
    lease.update(Rect { width: 0, ..rect });
    lease.update(rect);
    assert!(lease.snapshot().is_none());
}

#[test]
fn actual_held_modifiers_block_shortcut_spoofing_and_releases_remain_possible() {
    let (lease, rect) = geometry();
    let mut owner = owner();
    for vk in [0x11, 0x12] {
        owner
            .input(
                &lease,
                rect,
                Packet::Key {
                    vk,
                    down: true,
                    modifiers: 0,
                },
                false,
            )
            .unwrap();
    }
    owner
        .input(
            &lease,
            rect,
            Packet::Key {
                vk: 0x2e,
                down: true,
                modifiers: 0,
            },
            false,
        )
        .unwrap();
    assert!(!owner.keys.contains(&0x2e));
    owner
        .input(
            &lease,
            rect,
            Packet::Key {
                vk: 0x12,
                down: false,
                modifiers: 8,
            },
            true,
        )
        .unwrap();
    assert!(!owner.keys.contains(&0x12));
    owner.backend.physical = 4;
    owner
        .input(
            &lease,
            rect,
            Packet::Key {
                vk: 0x73,
                down: true,
                modifiers: 0,
            },
            false,
        )
        .unwrap();
    assert!(!owner.keys.contains(&0x73));
}

#[test]
fn foreground_loss_and_host_priority_do_not_block_owned_cleanup_releases() {
    let (lease, rect) = geometry();
    let mut owner = owner();
    owner
        .input(
            &lease,
            rect,
            Packet::Mouse {
                button: 0,
                down: true,
            },
            false,
        )
        .unwrap();
    owner
        .input(
            &lease,
            rect,
            Packet::Key {
                vk: 0x41,
                down: true,
                modifiers: 0,
            },
            false,
        )
        .unwrap();
    owner.backend.foreground = false;
    owner
        .input(
            &lease,
            rect,
            Packet::Mouse {
                button: 2,
                down: true,
            },
            false,
        )
        .unwrap();
    owner
        .input(
            &lease,
            rect,
            Packet::Key {
                vk: 0x42,
                down: true,
                modifiers: 0,
            },
            false,
        )
        .unwrap();
    owner
        .input(
            &lease,
            rect,
            Packet::Mouse {
                button: 0,
                down: false,
            },
            true,
        )
        .unwrap();
    owner
        .input(
            &lease,
            rect,
            Packet::Key {
                vk: 0x41,
                down: false,
                modifiers: 8,
            },
            true,
        )
        .unwrap();
    assert!(owner.keys.is_empty() && owner.buttons.is_empty());
    assert_eq!(owner.backend.calls.len(), 4);
}

#[test]
fn reset_releases_all_owned_inputs_and_failed_release_poison_is_not_hidden() {
    let (lease, rect) = geometry();
    let mut owner = owner();
    owner
        .input(
            &lease,
            rect,
            Packet::Key {
                vk: 0x41,
                down: true,
                modifiers: 0,
            },
            false,
        )
        .unwrap();
    owner
        .input(
            &lease,
            rect,
            Packet::Mouse {
                button: 1,
                down: true,
            },
            false,
        )
        .unwrap();
    owner.backend.fail_release = true;
    assert_eq!(owner.finish(), Err(InputError::Cleanup));
    assert!(owner.cleanup_failed.load(Ordering::Acquire));
    assert!(!owner.keys.is_empty() && !owner.buttons.is_empty());
    owner.backend.fail_release = false;
    owner.finish().unwrap();
    assert!(owner.keys.is_empty() && owner.buttons.is_empty());
    assert!(owner.cleanup_failed.load(Ordering::Acquire));
}

#[test]
fn cancelled_owner_does_not_emit_ready_or_deliver_queued_input() {
    let (lease, _) = geometry();
    let (send, recv) = mpsc::sync_channel(4);
    send.send(Packet::Mouse {
        button: 0,
        down: true,
    })
    .unwrap();
    let mut notified = false;
    drive(
        Fake::default(),
        lease,
        recv,
        || true,
        |_| {
            notified = true;
            true
        },
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(!notified);
}
