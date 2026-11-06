use bediz_core::capture::Allocation;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::{DISPLAY_SETTLE_MILLISECONDS, DISPLAY_TIMER, Runtime};
use crate::event::{Event, Failure};
use crate::windows::scene::Scene;

impl Runtime {
    pub(super) fn on_display_change(&mut self) {
        self.hide();
        self.displays_changed = true;
        let timer = unsafe {
            SetTimer(
                Some(self.window.0),
                DISPLAY_TIMER,
                DISPLAY_SETTLE_MILLISECONDS,
                None,
            )
        };
        if timer == 0 {
            self.rebuild_scene();
        }
    }

    pub(super) fn rebuild_scene(&mut self) {
        let _ = unsafe { KillTimer(Some(self.window.0), DISPLAY_TIMER) };
        self.displays_changed = false;
        self.build_scene();
        self.report_idle();
    }

    pub(super) fn build_scene(&mut self) {
        self.scene = None;
        let mut notes = Vec::new();
        match Scene::new(self.instance, self.allocation, &mut notes) {
            Ok(scene) => self.scene = Some(scene),
            Err(error) => (self.sink)(Event::Failed(Failure::Displays(error.to_string()))),
        }
        self.emit_notes(notes);
    }

    pub(super) fn set_allocation(&mut self, allocation: Allocation) {
        if allocation == self.allocation {
            return;
        }
        self.hide();
        self.allocation = allocation;
        if let Some(scene) = self.scene.as_mut() {
            let result = match allocation {
                Allocation::Ahead => scene.allocate().and_then(|()| scene.warm_up()),
                Allocation::OnHotkey => scene.release(),
            };
            if let Err(error) = result {
                (self.sink)(Event::Failed(Failure::Overlay(error.to_string())));
            }
        }
        self.report_idle();
    }
}
