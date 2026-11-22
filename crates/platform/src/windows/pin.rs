//! A screenshot pinned on top of every window: its window, which never takes the focus by itself,
//! zoomed and faded with its border outside the image, and the pixels it keeps.

mod composed;
mod devices;
mod input;
pub mod menu;

pub use devices::Devices;

use std::sync::Arc;

use bediz_core::geometry::{Point, Rect, length};
use bediz_core::image::Image;
use bediz_core::pin::{self, Detent, Look, Wheel};
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::Graphics::Dwm::{
    DWMWA_TRANSITIONS_FORCEDISABLED, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DWMWINDOWATTRIBUTE, DwmSetWindowAttribute,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER,
    SetForegroundWindow, SetWindowDisplayAffinity, SetWindowPos, SetWindowTextW, ShowWindow,
    WDA_EXCLUDEFROMCAPTURE, WDA_NONE, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{BOOL, HSTRING, Result};

use super::contrast::{self, high_contrast};
use super::monitor::{Monitor, scale_of};
use super::window_class::{OwnedWindow, PIN_CLASS};
use super::worker::{Pixels, Png};
use composed::Composed;
use input::Press;

const QUIET_BORDER: [u8; 4] = [0x80, 0x80, 0x80, 0xFF];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinEvent {
    Activated(bool),
    Rescaled,
    Painted,
    Close,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Asked {
    #[default]
    Nothing,
    Close,
    Copy,
    Save,
    SaveAs,
    Menu(Point),
}

enum Kept {
    Raw(Arc<Image>),
    Png(Png),
}

pub struct Pin {
    id: u64,
    window: OwnedWindow,
    size: (u32, u32),
    kept: Kept,
    at: Point,
    home: Point,
    look: Look,
    made: Look,
    border: u32,
    focused: bool,
    composed: Option<Composed>,
    press: Option<Press>,
    zoom_wheel: Wheel,
    fade_wheel: Wheel,
    detent: Detent,
}

impl Pin {
    pub fn create(
        instance: HINSTANCE,
        id: u64,
        image: Arc<Image>,
        over: Rect,
        title: &str,
        in_captures: bool,
    ) -> Result<Self> {
        let size = (image.width(), image.height());
        let at = Point::new(over.left, over.top);
        let border = pin::border(Monitor::at(at).scale());
        let look = Look::fitted(size, (over.width(), over.height()));
        let rect = pin::window(at, pin::shown(size, look.zoom), border);
        let window = OwnedWindow(unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
                PIN_CLASS,
                &HSTRING::from(title),
                WS_POPUP,
                rect.left,
                rect.top,
                length(rect.width()),
                length(rect.height()),
                None,
                None,
                Some(instance),
                None,
            )
        }?);
        set_attribute(window.0, DWMWA_TRANSITIONS_FORCEDISABLED, BOOL::from(true))?;
        let _ = set_attribute(window.0, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND);
        let mut made = Self {
            id,
            window,
            size,
            kept: Kept::Raw(image),
            at,
            home: at,
            look,
            made: look,
            border,
            focused: false,
            composed: None,
            press: None,
            zoom_wheel: Wheel::default(),
            fade_wheel: Wheel::default(),
            detent: Detent::default(),
        };
        made.border = made.own_border();
        made.show_in_captures(in_captures)?;
        Ok(made)
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn window(&self) -> HWND {
        self.window.0
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn rect(&self) -> Rect {
        pin::window(self.at, self.shown(), self.border)
    }

    pub fn look(&self) -> Look {
        self.look
    }

    pub fn can_zoom(&self, steps: i32) -> bool {
        self.look.zoomed(steps, false, self.size, self.room()) != self.look
    }

    pub fn fills_monitor(&self) -> bool {
        self.look.fills(self.size, self.room())
    }

    pub fn is_actual_size(&self) -> bool {
        self.look.actual_size(self.size) == self.look
    }

    pub fn pixels(&self) -> Pixels {
        match &self.kept {
            Kept::Raw(image) => Pixels::Image(Arc::clone(image)),
            Kept::Png(png) => Pixels::Png(Arc::clone(png)),
        }
    }

    pub fn keep(&mut self, png: Png) {
        self.kept = Kept::Png(png);
    }

    pub fn retitle(&self, title: &str) -> Result<()> {
        unsafe { SetWindowTextW(self.window.0, &HSTRING::from(title)) }
    }

    pub fn show_in_captures(&self, shown: bool) -> Result<()> {
        let affinity = if shown {
            WDA_NONE
        } else {
            WDA_EXCLUDEFROMCAPTURE
        };
        unsafe { SetWindowDisplayAffinity(self.window.0, affinity) }
    }

    pub fn is_composed(&self) -> bool {
        self.composed.is_some()
    }

    // An upload keeps the driver's buffers until a commit, which a pin waiting hidden would not make.
    pub fn compose(&mut self, devices: &Devices, image: &Image) -> Result<()> {
        if self.composed.is_none() {
            let composed = Composed::create(devices, self.window.0, image)?;
            composed.paint_border(devices, self.border_colour())?;
            self.composed = Some(composed);
            devices.commit()?;
        }
        Ok(())
    }

    pub fn show(&mut self, devices: &Devices) -> Result<()> {
        self.border = self.own_border();
        self.come_home();
        self.lay_out(devices)?;
        let _ = unsafe { ShowWindow(self.window.0, SW_SHOWNOACTIVATE) };
        self.raise()
    }

    pub fn forget_composition(&mut self) {
        self.composed = None;
    }

    pub fn hide(&mut self) {
        self.release(false);
        self.focused = false;
        let _ = unsafe { ShowWindow(self.window.0, SW_HIDE) };
        self.composed = None;
    }

    pub fn raise(&self) -> Result<()> {
        let rect = self.rect();
        unsafe {
            SetWindowPos(
                self.window.0,
                Some(HWND_TOPMOST),
                rect.left,
                rect.top,
                length(rect.width()),
                length(rect.height()),
                SWP_NOACTIVATE,
            )
        }
    }

    pub fn focus(&self) {
        let _ = unsafe { SetForegroundWindow(self.window.0) };
    }

    pub fn activated(&mut self, devices: &Devices, focused: bool) -> Result<()> {
        self.focused = focused;
        self.repaint_border(devices)
    }

    pub fn repaint_border(&self, devices: &Devices) -> Result<()> {
        let Some(composed) = &self.composed else {
            return Ok(());
        };
        composed.paint_border(devices, self.border_colour())?;
        devices.commit()
    }

    pub fn rescale(&mut self, devices: &Devices) -> Result<()> {
        self.border = self.own_border();
        self.lay_out(devices)
    }

    pub fn follow_monitors(&mut self, devices: &Devices) -> Result<()> {
        self.come_home();
        self.lay_out(devices)
    }

    // The place the user gave a pin is kept while its monitor is gone, for when it returns.
    fn come_home(&mut self) {
        let home = pin::window(self.home, self.shown(), self.border);
        let nearest = bounds_of(Monitor::near(home));
        self.at = pin::image_at(pin::kept_on(home, &[nearest]), self.border);
    }

    pub fn change_look(
        &mut self,
        devices: &Devices,
        look: Look,
        about: Option<Point>,
    ) -> Result<()> {
        if look == self.look {
            return Ok(());
        }
        // Only a zoom moves the pin; a fade keeps the place it has for a monitor that is gone.
        if look.zoom != self.look.zoom {
            let centre = pin::zoom_centre(self.at, self.shown(), about);
            self.at = pin::zoomed_about(self.at, self.look.zoom, look.zoom, centre);
            if look.fills(self.size, self.room()) {
                let shown = pin::shown(self.size, look.zoom);
                self.at = pin::inside(self.at, shown, self.monitor());
            }
            self.home = self.at;
        }
        self.look = look;
        self.lay_out(devices)
    }

    pub fn zoom(
        &mut self,
        devices: &Devices,
        steps: i32,
        fine: bool,
        about: Option<Point>,
    ) -> Result<()> {
        let look = self.look.turned(steps, fine, self.size, self.room());
        self.change_look(devices, look, about)
    }

    pub fn fill(&mut self, devices: &Devices) -> Result<()> {
        let look = self.look.filled(self.size, self.room());
        self.change_look(devices, look, None)
    }

    pub fn show_actual_size(&mut self, devices: &Devices) -> Result<()> {
        let look = self.look.actual_size(self.size);
        self.change_look(devices, look, None)
    }

    pub fn middle(&self) -> Point {
        pin::zoom_centre(self.at, self.shown(), None)
    }

    fn lay_out(&self, devices: &Devices) -> Result<()> {
        let rect = self.rect();
        if let Some(composed) = &self.composed {
            composed.lay_out(self.look, (rect.width(), rect.height()), self.border)?;
            devices.commit()?;
        }
        unsafe {
            SetWindowPos(
                self.window.0,
                None,
                rect.left,
                rect.top,
                length(rect.width()),
                length(rect.height()),
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        }
    }

    fn shown(&self) -> (u32, u32) {
        pin::shown(self.size, self.look.zoom)
    }

    // The border may hang off the monitor's edge, so a pin filling its monitor keeps its pixels.
    fn room(&self) -> (u32, u32) {
        let monitor = self.monitor();
        (monitor.width(), monitor.height())
    }

    fn monitor(&self) -> Rect {
        bounds_of(Monitor::of(self.window.0))
    }

    fn own_border(&self) -> u32 {
        pin::border(scale_of(unsafe { GetDpiForWindow(self.window.0) }))
    }

    fn border_colour(&self) -> [u8; 4] {
        if self.focused {
            contrast::highlight_bgra()
        } else if high_contrast() {
            contrast::text_bgra()
        } else {
            QUIET_BORDER
        }
    }
}

fn bounds_of(monitor: Monitor) -> Rect {
    monitor
        .bounds()
        .unwrap_or(Rect::new(0, 0, i32::MAX, i32::MAX))
}

fn set_attribute<T>(window: HWND, attribute: DWMWINDOWATTRIBUTE, value: T) -> Result<()> {
    unsafe {
        DwmSetWindowAttribute(
            window,
            attribute,
            (&value as *const T).cast(),
            size_of::<T>() as u32,
        )
    }
}
