pub mod drag;
mod fonts;
mod layout;
pub mod paint;
mod tooltips;

use std::ffi::c_void;

use bediz_core::geometry::Rect;
use bediz_core::recording;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::HFONT;
use windows::Win32::System::SystemServices::{SS_CENTER, SS_CENTERIMAGE};
use windows::Win32::UI::WindowsAndMessaging::{
    BS_OWNERDRAW, CreateWindowExW, HMENU, HWND_TOPMOST, IsChild, SW_HIDE, SW_SHOWNA,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SendMessageW, SetWindowDisplayAffinity,
    SetWindowPos, SetWindowTextW, ShowWindow, USER_DEFAULT_SCREEN_DPI, WDA_EXCLUDEFROMCAPTURE,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_SETFONT, WS_CHILD, WS_CLIPCHILDREN, WS_EX_LAYOUTRTL,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

use crate::event::MenuTexts;
use crate::windows::window_class::{CONTROLS_CLASS, MOVABLE_CONTROLS_CLASS, OwnedWindow};
use fonts::{Font, icon_font, measure, message_font};
use layout::{Content, layout};

const DISTANCE: f32 = 10.0;
const LONGEST_TIME: &str = "00:00";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Ready,
    Recording,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Start = 1,
    Pause = 2,
    Resume = 3,
    Finish = 4,
    Restart = 5,
    Cancel = 6,
    Discard = 7,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Press(Button),
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Face {
    Labelled,
    Square,
    Glyph(u16),
}

impl Stage {
    fn slots(self) -> &'static [&'static [Button]] {
        match self {
            Self::Ready => &[&[Button::Start], &[Button::Cancel]],
            Self::Recording => &[
                &[Button::Pause, Button::Resume],
                &[Button::Finish],
                &[Button::Restart],
                &[Button::Discard],
            ],
        }
    }

    fn movable(self) -> bool {
        self == Self::Recording
    }
}

impl Button {
    const ALL: [Self; 7] = [
        Self::Start,
        Self::Pause,
        Self::Resume,
        Self::Finish,
        Self::Restart,
        Self::Cancel,
        Self::Discard,
    ];

    pub fn from_id(id: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|button| button.id() == id)
    }

    fn id(self) -> u16 {
        self as u16
    }

    fn face(self) -> Face {
        match self {
            Self::Start => Face::Labelled,
            Self::Finish => Face::Square,
            Self::Pause => Face::Glyph(0xE769),
            Self::Resume => Face::Glyph(0xE768),
            Self::Restart => Face::Glyph(0xE72C),
            Self::Cancel => Face::Glyph(0xE711),
            Self::Discard => Face::Glyph(0xE74D),
        }
    }

    fn text(self, texts: &MenuTexts) -> &str {
        match self {
            Self::Start => &texts.start_recording,
            Self::Pause => &texts.pause_recording,
            Self::Resume => &texts.resume_recording,
            Self::Finish => &texts.finish_recording,
            Self::Restart => &texts.restart_recording,
            Self::Cancel => &texts.cancel_recording,
            Self::Discard => &texts.discard_recording,
        }
    }
}

pub struct Controls {
    window: OwnedWindow,
    time: Option<HWND>,
    pause: Option<(HWND, HWND)>,
    _font: Font,
    _icons: Option<Font>,
    _tips: Vec<Vec<u16>>,
    dpi: u32,
    width: u32,
    height: u32,
}

impl Controls {
    pub fn create(
        instance: HINSTANCE,
        stage: Stage,
        area: Rect,
        work: Rect,
        dpi: u32,
        texts: &MenuTexts,
    ) -> Result<Self> {
        let scale = scale(dpi);
        let font = message_font(dpi)?;
        let icons = icon_font(dpi);
        let slots = stage.slots();
        let contents: Vec<Content> = slots
            .iter()
            .map(|slot| content(slot, &font, icons.is_some(), texts))
            .collect();
        let time_width = (stage == Stage::Recording).then(|| {
            measure(&font, &[LONGEST_TIME])
                .first()
                .copied()
                .unwrap_or(0)
        });
        let layout = layout(scale, stage.movable(), time_width, &contents);
        let width = u32::try_from(layout.width).unwrap_or(0);
        let height = u32::try_from(layout.height).unwrap_or(0);
        let at = recording::controls_at(area, work, width, height, pixels(DISTANCE, scale));
        let (title, class) = match stage {
            Stage::Ready => (&texts.record, CONTROLS_CLASS),
            Stage::Recording => (&texts.recording, MOVABLE_CONTROLS_CLASS),
        };
        let mut extended = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        if texts.right_to_left {
            extended |= WS_EX_LAYOUTRTL;
        }
        let window = OwnedWindow(unsafe {
            CreateWindowExW(
                extended,
                class,
                &HSTRING::from(title.as_str()),
                WS_POPUP | WS_CLIPCHILDREN,
                at.x,
                at.y,
                layout.width,
                layout.height,
                None,
                None,
                Some(instance),
                None,
            )
        }?);
        let time = match layout.time {
            Some(rect) => Some(child(
                instance,
                window.0,
                w!("STATIC"),
                &recording::clock_text(std::time::Duration::ZERO),
                WINDOW_STYLE(SS_CENTER.0 | SS_CENTERIMAGE.0) | WS_VISIBLE,
                rect,
                0,
            )?),
            None => None,
        };
        let owner_drawn = WINDOW_STYLE(BS_OWNERDRAW as u32);
        let mut fonts: Vec<(HWND, HFONT)> = time.map(|time| (time, font.0)).into_iter().collect();
        let mut tools: Vec<(HWND, &str)> = Vec::new();
        let (mut pause, mut resume) = (None, None);
        for (slot, rect) in slots.iter().zip(&layout.buttons) {
            for (index, &button) in slot.iter().enumerate() {
                let style = if index == 0 {
                    owner_drawn | WS_VISIBLE
                } else {
                    owner_drawn
                };
                let created = child(
                    instance,
                    window.0,
                    w!("BUTTON"),
                    button.text(texts),
                    style,
                    *rect,
                    button.id(),
                )?;
                let chosen = match (button.face(), &icons) {
                    (Face::Glyph(_), Some(icons)) => icons.0,
                    _ => font.0,
                };
                fonts.push((created, chosen));
                if button.face() != Face::Labelled {
                    tools.push((created, button.text(texts)));
                }
                match button {
                    Button::Pause => pause = Some(created),
                    Button::Resume => resume = Some(created),
                    _ => {}
                }
            }
        }
        for (control, chosen) in fonts {
            unsafe {
                SendMessageW(
                    control,
                    WM_SETFONT,
                    Some(WPARAM(chosen.0 as usize)),
                    Some(LPARAM(0)),
                )
            };
        }
        unsafe { SetWindowDisplayAffinity(window.0, WDA_EXCLUDEFROMCAPTURE) }?;
        let corner = DWMWCP_ROUND;
        let _ = unsafe {
            DwmSetWindowAttribute(
                window.0,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&corner as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
                size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            )
        };
        let tips =
            tooltips::add(instance, window.0, texts.right_to_left, &tools).unwrap_or_default();
        Ok(Self {
            window,
            time,
            pause: pause.zip(resume),
            _font: font,
            _icons: icons,
            _tips: tips,
            dpi,
            width,
            height,
        })
    }

    pub fn window(&self) -> HWND {
        self.window.0
    }

    pub fn dpi(&self) -> u32 {
        self.dpi
    }

    pub fn owns(&self, window: HWND) -> bool {
        window == self.window.0 || unsafe { IsChild(self.window.0, window) }.as_bool()
    }

    pub fn reveal(&self) -> Result<()> {
        unsafe {
            SetWindowPos(
                self.window.0,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        }
    }

    pub fn place(&self, area: Rect, work: Rect) -> Result<()> {
        let distance = pixels(DISTANCE, scale(self.dpi));
        let at = recording::controls_at(area, work, self.width, self.height, distance);
        unsafe {
            SetWindowPos(
                self.window.0,
                Some(HWND_TOPMOST),
                at.x,
                at.y,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        }
    }

    pub fn hide(&self) {
        let _ = unsafe { ShowWindow(self.window.0, SW_HIDE) };
    }

    pub fn show_time(&self, text: &str) -> Result<()> {
        match self.time {
            Some(time) => unsafe { SetWindowTextW(time, &HSTRING::from(text)) },
            None => Ok(()),
        }
    }

    pub fn show_paused(&self, paused: bool) {
        let Some((pause, resume)) = self.pause else {
            return;
        };
        let (shown, hidden) = if paused {
            (resume, pause)
        } else {
            (pause, resume)
        };
        unsafe {
            let _ = ShowWindow(hidden, SW_HIDE);
            let _ = ShowWindow(shown, SW_SHOWNA);
        }
    }
}

fn content(slot: &[Button], font: &Font, icons: bool, texts: &MenuTexts) -> Content {
    let widest = || {
        let labels: Vec<&str> = slot.iter().map(|button| button.text(texts)).collect();
        measure(font, &labels).into_iter().max().unwrap_or(0)
    };
    match slot.first().map(|button| button.face()) {
        Some(Face::Labelled) => Content::Text {
            width: widest(),
            marked: true,
        },
        Some(Face::Glyph(_)) if !icons => Content::Text {
            width: widest(),
            marked: false,
        },
        Some(Face::Square | Face::Glyph(_)) | None => Content::Icon,
    }
}

fn child(
    instance: HINSTANCE,
    parent: HWND,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    at: Rect,
    id: u16,
) -> Result<HWND> {
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            &HSTRING::from(text),
            WS_CHILD | style,
            at.left,
            at.top,
            i32::try_from(at.width()).unwrap_or(0),
            i32::try_from(at.height()).unwrap_or(0),
            Some(parent),
            Some(HMENU(usize::from(id) as *mut c_void)),
            Some(instance),
            None,
        )
    }
}

fn scale(dpi: u32) -> f32 {
    dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32
}

fn pixels(value: f32, scale: f32) -> i32 {
    (value * scale).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_button_is_found_by_its_id() {
        for button in Button::ALL {
            assert_eq!(Button::from_id(button.id()), Some(button));
        }
        assert_eq!(Button::from_id(0), None);
        assert_eq!(Button::from_id(8), None);
    }

    #[test]
    fn the_stages_offer_their_buttons() {
        assert_eq!(
            Stage::Ready.slots(),
            [[Button::Start].as_slice(), [Button::Cancel].as_slice()]
        );
        assert_eq!(
            Stage::Recording.slots(),
            [
                [Button::Pause, Button::Resume].as_slice(),
                [Button::Finish].as_slice(),
                [Button::Restart].as_slice(),
                [Button::Discard].as_slice(),
            ]
        );
        assert!(Stage::Recording.movable());
        assert!(!Stage::Ready.movable());
    }

    #[test]
    fn only_start_shows_its_text_and_the_rest_show_icons() {
        for button in Button::ALL {
            let labelled = button.face() == Face::Labelled;
            assert_eq!(labelled, button == Button::Start, "{button:?}");
        }
        assert_eq!(Button::Finish.face(), Face::Square);
        assert_eq!(Button::Pause.face(), Face::Glyph(0xE769));
        assert_eq!(Button::Resume.face(), Face::Glyph(0xE768));
    }
}
