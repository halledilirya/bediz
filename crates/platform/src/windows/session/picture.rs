//! The selection of a picture opened to mark up: it is the picture itself, it never moves or
//! changes size, its blurs and pixelations are made from the picture alone, and the size it
//! reports is the picture's own.

use bediz_core::delivery::Delivery;
use bediz_core::geometry::{Point, Rect};
use bediz_core::image::Image;
use bediz_core::selection::{Grip, Selection};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::IDC_ARROW;

use super::{Purpose, Session};
use crate::windows::cursor;
use crate::windows::scene::Scene;

pub struct Picture {
    pub image: Image,
    pub shown: Rect,
}

impl Session {
    pub fn for_picture(
        previous: HWND,
        enter: Delivery,
        scene: &Scene,
        reply_to: HWND,
        picture: Picture,
    ) -> Self {
        let mut session = Self::new(previous, enter, Purpose::Picture, scene, reply_to);
        session.selection = Selection::restore(scene.monitor_rects(), picture.shown);
        session.marking.make_effects_from(picture.shown);
        session.picture = Some(picture);
        cursor::show(IDC_ARROW);
        session
    }

    pub fn picture(&self) -> Option<&Picture> {
        self.picture.as_ref()
    }

    pub fn size_of(&self, rect: Rect) -> (u32, u32) {
        match &self.picture {
            Some(picture) => (picture.image.width(), picture.image.height()),
            None => (rect.width(), rect.height()),
        }
    }

    pub fn settled_size(&self) -> Option<(u32, u32)> {
        self.settled().map(|rect| self.size_of(rect))
    }

    pub(super) fn grip(&self, selection: &Selection, at: Point, reach: i32) -> Option<Grip> {
        if !self.purpose.reshapes() {
            return selection
                .rect()
                .filter(|rect| rect.contains(at))
                .map(|_| Grip::Inside);
        }
        selection.grip_at(at, reach)
    }
}
