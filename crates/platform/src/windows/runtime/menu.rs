//! The overlay's right-click menu and what each choice does, and the screenshot taken from the
//! tray menu once the menu has faded.

use std::time::Duration;

use bediz_core::geometry::Point;
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETSELECTIONFADE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetTimer, SystemParametersInfoW,
};
use windows::core::BOOL;

use super::{MENU_FADE_TIMER, Runtime};
use crate::event::{Event, Failure};
use crate::windows::overlay_menu::{self, Choice, Offer};
use crate::windows::session::Purpose;

const MENU_FADE_MILLISECONDS: u32 = 500;

impl Runtime {
    // The clicked menu item fades out for a moment and would be in the capture.
    pub(super) fn open_after_menu(&mut self) {
        self.cancel_delay();
        let timer = menu_fades()
            && unsafe {
                SetTimer(
                    Some(self.window.0),
                    MENU_FADE_TIMER,
                    MENU_FADE_MILLISECONDS,
                    None,
                )
            } != 0;
        if timer {
            self.menu_capture = true;
        } else {
            self.open(Duration::ZERO, Purpose::Screenshot);
        }
    }

    pub(super) fn open_menu(&mut self, at: Point) {
        let (Some(texts), Some(scene), Some(session)) = (
            self.menu_texts.as_ref(),
            self.scene.as_ref(),
            self.session.as_ref(),
        ) else {
            return;
        };
        let Some(owner) = scene.window_at(at) else {
            return;
        };
        let offer = Offer {
            selected: session.selected().is_some(),
            last_region: self.last_region.is_some(),
            enter: self.enter,
            recording: session.purpose() == Purpose::Recording,
            picture: session.purpose() == Purpose::Picture,
        };
        match overlay_menu::choose(owner, at, texts, &offer) {
            Ok(Some(Choice::Deliver(delivery))) => self.deliver(delivery),
            Ok(Some(Choice::SaveAs)) => self.save_as(),
            Ok(Some(Choice::ReadText)) => self.read_text(),
            Ok(Some(Choice::Record)) => self.record(menu_fade()),
            Ok(Some(Choice::CopyColour)) => self.copy_colour(at),
            Ok(Some(Choice::WholeMonitor)) => {
                self.reselect(|session, scene| session.select_whole_monitor(scene, at));
            }
            Ok(Some(Choice::LastRegion)) => {
                if let Some(region) = self.last_region {
                    self.reselect(|session, scene| session.select_region(scene, region));
                }
            }
            Ok(Some(Choice::Cancel)) => self.hide(),
            Ok(None) => {}
            Err(error) => (self.sink)(Event::Failed(Failure::Overlay(error.to_string()))),
        }
    }
}

fn menu_fade() -> Duration {
    if menu_fades() {
        Duration::from_millis(u64::from(MENU_FADE_MILLISECONDS))
    } else {
        Duration::ZERO
    }
}

fn menu_fades() -> bool {
    let mut fades = BOOL(0);
    unsafe {
        SystemParametersInfoW(
            SPI_GETSELECTIONFADE,
            0,
            Some((&mut fades as *mut BOOL).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok_and(|()| fades.as_bool())
}
