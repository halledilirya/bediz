//! The seconds left before a recording starts or a delayed screenshot is taken, shown and
//! announced to screen readers.

use std::time::Duration;

use bediz_core::countdown;
use bediz_core::geometry::Rect;
use bediz_core::recording::COUNTDOWN;
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::core::Result;

use crate::event::Label;
use crate::windows::contrast::Colours;
use crate::windows::label_window::LabelWindow;

pub struct Countdown {
    label: LabelWindow,
    near: Rect,
    colours: Option<Colours>,
    large: bool,
    shown: u64,
}

impl Countdown {
    pub fn open(instance: HINSTANCE, area: Rect, colours: Option<Colours>) -> Result<Self> {
        Self::start(instance, area, colours, true, COUNTDOWN)
    }

    pub fn in_corner(
        instance: HINSTANCE,
        corner: Rect,
        colours: Option<Colours>,
        remaining: Duration,
    ) -> Result<Self> {
        Self::start(instance, corner, colours, false, remaining)
    }

    fn start(
        instance: HINSTANCE,
        near: Rect,
        colours: Option<Colours>,
        large: bool,
        remaining: Duration,
    ) -> Result<Self> {
        let mut started = Self {
            label: LabelWindow::create(instance)?,
            near,
            colours,
            large,
            shown: 0,
        };
        started.show(countdown::number(remaining))?;
        Ok(started)
    }

    pub fn is(&self, window: HWND) -> bool {
        self.label.is(window)
    }

    pub fn show(&mut self, number: u64) -> Result<()> {
        if number == self.shown {
            return Ok(());
        }
        let label = Label {
            lines: vec![number.to_string()],
            near: self.near,
            warning: false,
            language: "en".to_owned(),
            right_to_left: false,
        };
        let shown = if self.large {
            self.label.show_large(&label, self.colours.as_ref())?
        } else {
            self.label.show(&label, self.colours.as_ref())?
        };
        self.shown = number;
        self.label.announce(&label, shown)
    }
}

impl Drop for Countdown {
    fn drop(&mut self) {
        let _ = self.label.hide();
    }
}
