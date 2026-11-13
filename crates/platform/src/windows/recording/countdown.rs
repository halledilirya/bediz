//! The large countdown shown in the middle of the area before a recording starts, and announced to
//! screen readers.

use bediz_core::geometry::Rect;
use bediz_core::recording::{self, COUNTDOWN};
use windows::Win32::Foundation::HINSTANCE;
use windows::core::Result;

use crate::event::Label;
use crate::windows::contrast::Colours;
use crate::windows::label_window::LabelWindow;

pub struct Countdown {
    label: LabelWindow,
    area: Rect,
    colours: Option<Colours>,
    shown: u64,
}

impl Countdown {
    pub fn open(instance: HINSTANCE, area: Rect, colours: Option<Colours>) -> Result<Self> {
        let mut countdown = Self {
            label: LabelWindow::create(instance)?,
            area,
            colours,
            shown: 0,
        };
        countdown.show(recording::countdown_number(COUNTDOWN))?;
        Ok(countdown)
    }

    pub fn show(&mut self, number: u64) -> Result<()> {
        if number == self.shown {
            return Ok(());
        }
        let label = Label {
            lines: vec![number.to_string()],
            near: self.area,
            warning: false,
            language: "en".to_owned(),
            right_to_left: false,
        };
        let shown = self.label.show_large(&label, self.colours.as_ref())?;
        self.shown = number;
        self.label.announce(&label, shown)
    }
}

impl Drop for Countdown {
    fn drop(&mut self) {
        let _ = self.label.hide();
    }
}
