//! Makes blurs and pixelations on a thread of their own and hands each back to the platform
//! thread, so the overlay keeps answering while a large one is made.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use bediz_core::image::Image;
use bediz_core::markup::Mark;
use bediz_core::markup::effect::{self, Effect};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::windows::runtime::WM_EFFECT_MADE;

pub struct Order {
    pub mark: Mark,
    pub source: Image,
}

pub struct Maker {
    orders: Option<Sender<Order>>,
    reply: Reply,
}

#[derive(Clone)]
struct Reply {
    made: Arc<Mutex<Option<Option<Effect>>>>,
    window: isize,
}

impl Maker {
    pub fn new(reply_to: HWND) -> Self {
        Self {
            orders: None,
            reply: Reply {
                made: Arc::default(),
                window: reply_to.0 as isize,
            },
        }
    }

    pub fn order(&mut self, order: Order) {
        if self.orders.is_none() {
            self.orders = self.start();
        }
        let refused = match &self.orders {
            Some(orders) => orders.send(order).err().map(|refused| refused.0),
            None => Some(order),
        };
        if let Some(order) = refused {
            self.orders = None;
            self.reply.make(order);
        }
    }

    pub fn take(&self) -> Option<Option<Effect>> {
        self.reply
            .made
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    fn start(&self) -> Option<Sender<Order>> {
        let (orders, waiting) = mpsc::channel::<Order>();
        let reply = self.reply.clone();
        std::thread::Builder::new()
            .name("bediz-effects".to_owned())
            .spawn(move || {
                for order in waiting {
                    reply.make(order);
                }
            })
            .ok()
            .map(|_| orders)
    }
}

impl Reply {
    fn make(&self, order: Order) {
        let made = catch_unwind(AssertUnwindSafe(|| {
            effect::apply(&order.mark, &order.source)
        }))
        .ok()
        .flatten();
        *self.made.lock().unwrap_or_else(PoisonError::into_inner) = Some(made);
        let window = HWND(self.window as *mut c_void);
        let _ = unsafe { PostMessageW(Some(window), WM_EFFECT_MADE, WPARAM(0), LPARAM(0)) };
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bediz_core::geometry::Point;
    use bediz_core::markup::{Colour, Shape};

    use super::*;

    #[test]
    fn an_ordered_blur_is_made_on_its_own_thread_and_handed_back_once() {
        let mut maker = Maker::new(HWND::default());
        let mark = Mark {
            id: 7,
            shape: Shape::Blur(Point::new(0, 0), Point::new(9, 9)),
            colour: Colour::new(0, 0, 0),
            width: 3.0,
        };
        let bgra: Vec<u8> = (0..400u32).map(|value| (value * 37 % 256) as u8).collect();
        let source = Image::from_bgra(10, 10, bgra).unwrap();
        let expected = effect::apply(&mark, &source);
        assert!(expected.is_some());
        maker.order(Order { mark, source });
        let mut made = None;
        for _ in 0..5000 {
            made = maker.take();
            if made.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(made, Some(expected));
        assert_eq!(maker.take(), None);
    }
}
