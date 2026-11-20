//! One selection on the overlays, for a screenshot, a recording or an opened picture: turns the
//! mouse and keys into the selection, its marks, the words chosen in it, the window under the
//! pointer and the action chosen, and redraws.

mod keys;
mod live;
mod marks;
mod picture;
mod pointer;
mod words;

pub use picture::Picture;
pub use words::{Content, Found};

use bediz_core::delivery::Delivery;
use bediz_core::geometry::{Point, Rect};
use bediz_core::markup::{Mark, Markup, Shape, shape};
use bediz_core::selection::{Modifiers, Selection};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::SystemServices::MK_LBUTTON;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, IDC_CROSS, LoadCursorW, MSG, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};
use windows::core::{PCWSTR, Result};

use super::marking::{Decor, Marking};
use super::overlay::View;
use super::scene::Scene;
use super::window_class;
use super::window_list::WindowList;
use live::LiveFrames;
use pointer::Click;
use words::Words;

const HANDLES_REACH: i32 = 32;
const WAS_DOWN: isize = 1 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Cancel,
    Deliver(Delivery),
    SaveAs,
    Record,
    CopyColour(Point),
    Menu(Point),
    CopyText { cut: bool },
    PasteText,
    ReadText,
    CopyWords,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Screenshot,
    Recording,
    Picture,
}

impl Purpose {
    pub fn is_still(self) -> bool {
        self != Self::Recording
    }

    pub fn shows_live_screen(self) -> bool {
        self != Self::Screenshot
    }

    pub fn reshapes(self) -> bool {
        self != Self::Picture
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPress {
    pub key: VIRTUAL_KEY,
    pub alt: bool,
    pub control: bool,
    pub shift: bool,
    pub repeat: bool,
}

pub struct Session {
    previous: HWND,
    enter: Delivery,
    purpose: Purpose,
    selection: Option<Selection>,
    click: Option<Click>,
    beside: Option<Point>,
    pointer: Option<Point>,
    views: Vec<View>,
    live: LiveFrames,
    marking: Marking,
    picture: Option<Picture>,
    words: Option<Words>,
    words_shown: u64,
}

impl Session {
    pub fn new(
        previous: HWND,
        enter: Delivery,
        purpose: Purpose,
        scene: &Scene,
        reply_to: HWND,
    ) -> Self {
        show_cursor(IDC_CROSS);
        let mut cursor = POINT::default();
        let pointer = unsafe { GetCursorPos(&mut cursor) }
            .ok()
            .map(|()| Point::new(cursor.x, cursor.y));
        let scale = pointer.map_or(1.0, |pointer| scene.scale_at(pointer));
        Self {
            previous,
            enter,
            purpose,
            selection: None,
            click: None,
            beside: None,
            pointer,
            views: Vec::new(),
            live: LiveFrames::default(),
            marking: Marking::new(scale, reply_to),
            picture: None,
            words: None,
            words_shown: 0,
        }
    }

    pub fn marking(&self) -> &Marking {
        &self.marking
    }

    pub fn marking_mut(&mut self) -> &mut Marking {
        &mut self.marking
    }

    pub fn previous(&self) -> HWND {
        self.previous
    }

    pub fn set_previous(&mut self, previous: HWND) {
        self.previous = previous;
    }

    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    pub fn repaint(
        &mut self,
        scene: &Scene,
        refreshed: &[usize],
        input_waiting: bool,
    ) -> Result<()> {
        let shown = self.live.arrived(refreshed, input_waiting);
        let decor = decor(&self.marking, self.words.as_ref());
        for index in shown {
            if let Some(view) = self.views.get(index) {
                scene.draw_one(index, view, &decor)?;
            }
        }
        Ok(())
    }

    pub fn selected(&self) -> Option<Rect> {
        self.selection?.rect()
    }

    pub fn settled(&self) -> Option<Rect> {
        if self.click.is_some() || self.is_dragging() {
            return None;
        }
        self.selected()
    }

    pub fn handle(
        &mut self,
        scene: &Scene,
        windows: Option<&WindowList>,
        last_region: Option<Rect>,
        message: &MSG,
    ) -> Result<Action> {
        let point = Point::new(message.pt.x, message.pt.y);
        if message.message == WM_MOUSEMOVE {
            self.pointer = Some(point);
        }
        let changed = match message.message {
            WM_LBUTTONDBLCLK
                if self.marking.tool().is_none()
                    && !self.shows_words()
                    && self.inside_selection(scene, point) =>
            {
                return Ok(confirm(self.purpose, self.enter));
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                if !self.press_words(scene, point) && !self.press_mark(scene, message.hwnd, point) {
                    self.press(scene, windows, point);
                }
                if self.selection.is_some() {
                    unsafe { SetCapture(message.hwnd) };
                }
                true
            }
            // Alt+Tab or the Win key can take the mouse mid-drag; the button then comes up elsewhere.
            WM_MOUSEMOVE
                if message.wParam.0 & MK_LBUTTON.0 as usize == 0
                    && (self.click.is_some()
                        || self.beside.is_some()
                        || self.is_dragging()
                        || self.marks_busy()
                        || self.choosing_words()) =>
            {
                let _ = unsafe { ReleaseCapture() };
                self.release(scene, message.hwnd, point)
            }
            WM_MOUSEMOVE => self.hover(scene, point),
            WM_LBUTTONUP => {
                let _ = unsafe { ReleaseCapture() };
                self.release(scene, message.hwnd, point)
            }
            WM_RBUTTONUP if !self.is_dragging() && !self.marks_busy() && !self.choosing_words() => {
                return Ok(Action::Menu(point));
            }
            WM_CHAR => self.type_unit(scene, message.hwnd, message.wParam.0 as u16),
            WM_MOUSEWHEEL => self.turn_wheel((message.wParam.0 >> 16) as u16 as i16),
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let press = KeyPress {
                    key: VIRTUAL_KEY(message.wParam.0 as u16),
                    alt: message.message == WM_SYSKEYDOWN,
                    control: is_down(VK_CONTROL),
                    shift: is_down(VK_SHIFT),
                    repeat: message.lParam.0 & WAS_DOWN != 0,
                };
                return self.press_key(scene, windows, last_region, press, point);
            }
            WM_KEYUP | WM_SYSKEYUP => self.modifiers_changed(VIRTUAL_KEY(message.wParam.0 as u16)),
            _ => false,
        };
        if changed {
            self.redraw(scene, windows)?;
        }
        Ok(Action::None)
    }

    pub fn press_key(
        &mut self,
        scene: &Scene,
        windows: Option<&WindowList>,
        last_region: Option<Rect>,
        press: KeyPress,
        point: Point,
    ) -> Result<Action> {
        match self.key(scene, last_region, press, point) {
            Some(Action::None) => self.redraw(scene, windows).map(|()| Action::None),
            Some(action) => Ok(action),
            None => Ok(Action::None),
        }
    }

    pub fn select_whole_monitor(&mut self, scene: &Scene, at: Point) {
        (self.click, self.beside) = (None, None);
        self.selection = Selection::whole_monitor(scene.monitor_rects(), at);
    }

    pub fn select_region(&mut self, scene: &Scene, region: Rect) {
        (self.click, self.beside) = (None, None);
        self.selection = Selection::restore(scene.monitor_rects(), region);
    }

    fn is_dragging(&self) -> bool {
        self.selection
            .is_some_and(|selection| selection.is_dragging())
    }

    pub fn redraw_all(&mut self, scene: &Scene, windows: Option<&WindowList>) -> Result<()> {
        self.views.clear();
        self.redraw(scene, windows)
    }

    pub fn redraw(&mut self, scene: &Scene, windows: Option<&WindowList>) -> Result<()> {
        self.marking.measure_texts(scene);
        self.marking.refresh_previews(scene);
        self.forget_stale_words();
        let monitors = scene.monitor_rects();
        self.views.resize(monitors.len(), View::default());
        let decor = decor(&self.marking, self.words.as_ref());
        let words = words::stamp(self.words.as_ref());
        let marks = self.marking.version();
        let selection = self.selected();
        let highlight = match (&self.click, selection) {
            (Some(click), _) => Some(click.window),
            (None, None) => self
                .pointer
                .zip(windows)
                .and_then(|(pointer, windows)| windows.window_at(pointer)),
            (None, Some(_)) => None,
        };
        let labelled = selection.or(highlight).and_then(|rect| {
            label_monitor(monitors, rect).map(|monitor| (monitor, self.size_of(rect)))
        });
        let magnified = self.magnified();
        let unclipped = unclipped_area(self.marking.markup());
        for (index, (bounds, drawn)) in monitors.iter().zip(self.views.iter_mut()).enumerate() {
            let shown_here = |rect: &Rect| bounds.intersection(rect).is_some();
            let view = View {
                selection: selection.filter(shown_here),
                handles: self.purpose.reshapes(),
                highlight: highlight.filter(shown_here),
                label: labelled
                    .filter(|(monitor, _)| *monitor == index)
                    .map(|(_, size)| size),
                pointer: magnified.filter(|pointer| bounds.contains(*pointer)),
                marks: shows_marks(*bounds, selection, unclipped).then_some(marks),
                words: selection.filter(shown_here).and(words),
            };
            if *drawn != view || self.live.owed(index) {
                scene.draw_one(index, &view, &decor)?;
                *drawn = view;
                self.live.drawn(index);
            }
        }
        Ok(())
    }

    fn magnified(&self) -> Option<Point> {
        match &self.selection {
            None => self.pointer,
            Some(selection) => selection.focus(),
        }
    }
}

fn confirm(purpose: Purpose, enter: Delivery) -> Action {
    match purpose {
        Purpose::Screenshot | Purpose::Picture => Action::Deliver(enter),
        Purpose::Recording => Action::Record,
    }
}

fn decor<'a>(marking: &'a Marking, words: Option<&'a Words>) -> Decor<'a> {
    Decor {
        words: words::rects(words),
        words_turn: words::turn(words),
        codes: words::code_rects(words),
        words_chosen: words::chosen(words),
        ..marking.decor()
    }
}

fn shows_marks(monitor: Rect, selection: Option<Rect>, unclipped: Option<Rect>) -> bool {
    [selection, unclipped]
        .into_iter()
        .flatten()
        .any(|area| monitor.intersection(&area).is_some())
}

// A selected mark's handles and a typed text's caret are drawn without the selection's clip.
fn unclipped_area(markup: &Markup) -> Option<Rect> {
    let composed = markup.composed().map(|composed| composed.mark);
    [markup.selected(), markup.editing(), composed.as_ref()]
        .into_iter()
        .flatten()
        .map(reach)
        .reduce(|all, area| all.union(&area))
}

fn reach(mark: &Mark) -> Rect {
    let mut by = HANDLES_REACH;
    // A text not measured yet is estimated, and full-width characters run past the estimate.
    if let Shape::Text {
        text, extent: None, ..
    } = &mark.shape
    {
        by = by.saturating_add(shape::text_extent(text, mark.width).0.ceil() as i32);
    }
    let bounds = shape::bounds(mark);
    Rect::new(
        bounds.left.saturating_sub(by),
        bounds.top.saturating_sub(by),
        bounds.right.saturating_add(by),
        bounds.bottom.saturating_add(by),
    )
}

fn label_monitor(monitors: &[Rect], rect: Rect) -> Option<usize> {
    let corner = Point::new(rect.left, rect.top);
    monitors
        .iter()
        .position(|monitor| monitor.contains(corner))
        .or_else(|| {
            monitors
                .iter()
                .position(|monitor| monitor.intersection(&rect).is_some())
        })
}

fn modifiers() -> Modifiers {
    Modifiers {
        square: is_down(VK_SHIFT),
        centered: is_down(VK_MENU),
    }
}

fn is_down(key: VIRTUAL_KEY) -> bool {
    let state = unsafe { GetKeyState(i32::from(key.0)) };
    state < 0
}

fn show_cursor(shape: PCWSTR) {
    if let Ok(cursor) = unsafe { LoadCursorW(None, shape) } {
        window_class::use_cursor(cursor);
    }
}

#[cfg(test)]
mod tests {
    use bediz_core::markup::composition::Composition;
    use bediz_core::markup::{Swatch, Tool};

    use super::*;

    const LEFT: Rect = Rect::new(0, 0, 1920, 1080);
    const RIGHT: Rect = Rect::new(1920, 0, 3840, 1080);

    #[test]
    fn marks_are_redrawn_only_on_the_monitors_they_can_show_on() {
        let selection = Some(Rect::new(100, 100, 1800, 900));
        assert!(shows_marks(LEFT, selection, None));
        assert!(!shows_marks(RIGHT, selection, None));
        let mut markup = Markup::default();
        markup.draw(Tool::Line, Point::new(1700, 500), Swatch::Red.colour(), 3.0);
        markup.finish(Point::new(1915, 500), false);
        assert!(shows_marks(RIGHT, selection, unclipped_area(&markup)));
        markup.deselect();
        assert_eq!(unclipped_area(&markup), None);
        assert!(!shows_marks(RIGHT, selection, unclipped_area(&markup)));
    }

    #[test]
    fn a_text_being_composed_reaches_as_far_as_full_width_characters_go() {
        let mut markup = Markup::default();
        markup.draw(Tool::Text, Point::new(1800, 500), Swatch::Red.colour(), 3.0);
        let units: Vec<u16> = "日本語日本語".encode_utf16().collect();
        markup.compose(Composition::from_input_method(
            &units,
            &[],
            &[],
            units.len(),
        ));
        let full_width = 1800 + 6 * shape::text_size(3.0).ceil() as i32;
        let reached = unclipped_area(&markup).map(|area| area.right);
        assert!(reached.is_some_and(|right| right >= full_width));
        assert!(shows_marks(RIGHT, None, unclipped_area(&markup)));
    }

    #[test]
    fn only_an_opened_picture_keeps_its_selection_and_shows_it_without_handles() {
        assert!(Purpose::Screenshot.reshapes());
        assert!(Purpose::Recording.reshapes());
        assert!(!Purpose::Picture.reshapes());
    }
}
