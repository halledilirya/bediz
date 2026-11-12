use std::collections::VecDeque;
use std::mem;
use std::time::Duration;

use super::change::{self, Area, Cut, Rgb};
use super::{AnimationError, codec, gif_size};
use crate::image::Image;

pub struct StoredFrame {
    pub(super) at: Duration,
    pub(super) canvas: (u16, u16),
    pub(super) area: Area,
    data: Box<[u8]>,
}

impl StoredFrame {
    pub(super) fn cut(&self) -> Option<Cut> {
        let pixel_count = usize::from(self.area.width) * usize::from(self.area.height);
        codec::decompress(&self.data, pixel_count)
    }

    fn cost(&self) -> usize {
        cost(self.data.len())
    }
}

pub struct FrameStore {
    width: u16,
    height: u16,
    budget: usize,
    used: usize,
    previous: Option<Vec<Rgb>>,
    spare: Vec<Rgb>,
    scratch: Vec<u8>,
    frames: VecDeque<StoredFrame>,
}

impl FrameStore {
    pub fn new(width: u32, height: u32, budget: usize) -> Result<Self, AnimationError> {
        let (width, height) = gif_size(width, height)?;
        Ok(Self {
            width,
            height,
            budget,
            used: 0,
            previous: None,
            spare: Vec::new(),
            scratch: Vec::new(),
            frames: VecDeque::new(),
        })
    }

    pub fn push(&mut self, image: &Image, at: Duration) -> Result<(), AnimationError> {
        if image.width() != u32::from(self.width) || image.height() != u32::from(self.height) {
            return Err(AnimationError::FrameSize {
                width: self.width,
                height: self.height,
                actual_width: image.width(),
                actual_height: image.height(),
            });
        }
        let mut current = mem::take(&mut self.spare);
        current.clear();
        current.extend(
            image
                .bgra()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|[blue, green, red, _]| [*red, *green, *blue]),
        );
        let area = match &self.previous {
            Some(previous) => change::changed_area(previous, &current, self.width),
            None => Some(Area::whole(self.width, self.height)),
        };
        let Some(area) = area else {
            self.spare = current;
            return Ok(());
        };
        let cut = change::cut(self.previous.as_deref(), &current, self.width, area);
        codec::compress(&cut, &mut self.scratch);
        let used = self.used.saturating_add(cost(self.scratch.len()));
        if used > self.budget {
            self.spare = current;
            return Err(AnimationError::Budget {
                budget: self.budget,
            });
        }
        self.frames.push_back(StoredFrame {
            at,
            canvas: (self.width, self.height),
            area,
            data: self.scratch.as_slice().into(),
        });
        self.used = used;
        if let Some(previous) = self.previous.replace(current) {
            self.spare = previous;
        }
        Ok(())
    }

    pub fn pop(&mut self) -> Option<StoredFrame> {
        let frame = self.frames.pop_front()?;
        self.used = self.used.saturating_sub(frame.cost());
        Some(frame)
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn waiting(&self) -> usize {
        self.frames.len()
    }
}

fn cost(data: usize) -> usize {
    mem::size_of::<StoredFrame>().saturating_add(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Rgb = [0, 0, 0];
    const RED: Rgb = [255, 0, 0];

    fn image(width: u32, height: u32, colour: impl Fn(u32, u32) -> Rgb) -> Image {
        let mut bgra = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let [red, green, blue] = colour(x, y);
                bgra.extend_from_slice(&[blue, green, red, 255]);
            }
        }
        Image::from_bgra(width, height, bgra).unwrap()
    }

    fn noise(seed: u32) -> Image {
        image(64, 64, |x, y| {
            let mut value =
                (x * 64 + y + 1).wrapping_mul(0x9e37_79b1) ^ seed.wrapping_mul(0x85eb_ca77);
            value ^= value >> 15;
            value = value.wrapping_mul(0x2c1b_3c6d);
            value ^= value >> 12;
            [value as u8, (value >> 8) as u8, (value >> 16) as u8]
        })
    }

    fn unlimited(width: u32, height: u32) -> FrameStore {
        FrameStore::new(width, height, usize::MAX).unwrap()
    }

    #[test]
    fn the_first_frame_is_whole_and_every_pixel_changed() {
        let first = image(3, 2, |x, _| if x == 1 { RED } else { BLACK });
        let mut store = unlimited(3, 2);
        store.push(&first, Duration::from_millis(40)).unwrap();
        let frame = store.pop().unwrap();
        assert_eq!(frame.at, Duration::from_millis(40));
        assert_eq!(frame.canvas, (3, 2));
        assert_eq!(frame.area, Area::whole(3, 2));
        let cut = frame.cut().unwrap();
        assert!(cut.changed.iter().all(|changed| *changed));
        assert_eq!(cut.pixels, [BLACK, RED, BLACK, BLACK, RED, BLACK]);
    }

    #[test]
    fn a_frame_identical_to_the_last_one_is_not_kept() {
        let black = image(4, 4, |_, _| BLACK);
        let mut store = unlimited(4, 4);
        store.push(&black, Duration::ZERO).unwrap();
        let used = store.used();
        store.push(&black, Duration::from_millis(100)).unwrap();
        assert_eq!(store.frames.len(), 1);
        assert_eq!(store.used(), used);
    }

    #[test]
    fn a_later_frame_keeps_only_the_rectangle_that_changed() {
        let before = image(5, 4, |_, _| BLACK);
        let after = image(5, 4, |x, y| {
            if (x, y) == (1, 1) || (x, y) == (3, 2) {
                RED
            } else {
                BLACK
            }
        });
        let mut store = unlimited(5, 4);
        store.push(&before, Duration::ZERO).unwrap();
        store.push(&after, Duration::from_millis(70)).unwrap();
        store.pop().unwrap();
        let frame = store.pop().unwrap();
        assert_eq!(
            frame.area,
            Area {
                left: 1,
                top: 1,
                width: 3,
                height: 2
            }
        );
        let cut = frame.cut().unwrap();
        assert_eq!(cut.changed, [true, false, false, false, false, true]);
        assert_eq!(cut.pixels[0], RED);
        assert_eq!(cut.pixels[5], RED);
    }

    #[test]
    fn frames_come_out_in_order_and_give_back_their_bytes() {
        let mut store = unlimited(64, 64);
        for (n, millis) in [0, 67, 133].into_iter().enumerate() {
            store
                .push(&noise(n as u32), Duration::from_millis(millis))
                .unwrap();
        }
        assert_eq!(store.waiting(), 3);
        let times: Vec<Duration> = std::iter::from_fn(|| store.pop())
            .map(|frame| frame.at)
            .collect();
        assert_eq!(times, [0, 67, 133].map(Duration::from_millis));
        assert_eq!(store.used(), 0);
        assert_eq!(store.waiting(), 0);
        assert!(store.pop().is_none());
    }

    #[test]
    fn a_frame_past_the_budget_is_refused_and_leaves_the_store_as_it_was() {
        let black = image(64, 64, |_, _| BLACK);
        let mut measure = unlimited(64, 64);
        measure.push(&black, Duration::ZERO).unwrap();
        let budget = measure.used();
        let mut store = FrameStore::new(64, 64, budget).unwrap();
        store.push(&black, Duration::ZERO).unwrap();
        assert!(matches!(
            store.push(&noise(1), Duration::from_millis(100)),
            Err(AnimationError::Budget { budget: reached }) if reached == budget
        ));
        assert_eq!(store.frames.len(), 1);
        assert_eq!(store.used(), budget);
        store.push(&black, Duration::from_millis(200)).unwrap();
        assert_eq!(store.frames.len(), 1);
    }

    #[test]
    fn noise_fills_the_budget_and_stops_the_recording() {
        let budget = 64 * 1024;
        let mut store = FrameStore::new(64, 64, budget).unwrap();
        let mut stored = 0u32;
        let error = loop {
            match store.push(
                &noise(stored),
                Duration::from_millis(u64::from(stored) * 67),
            ) {
                Ok(()) => stored += 1,
                Err(error) => break error,
            }
            assert!(stored < 100, "the budget was never reached");
        };
        assert!(matches!(error, AnimationError::Budget { budget: reached } if reached == budget));
        assert!(stored > 0);
        assert!(store.used() <= budget);
        assert_eq!(store.frames.len(), stored as usize);
    }

    #[test]
    fn taking_frames_out_makes_room_again() {
        let mut measure = unlimited(64, 64);
        measure.push(&noise(1), Duration::ZERO).unwrap();
        let mut store = FrameStore::new(64, 64, measure.used() + 1024).unwrap();
        store.push(&noise(1), Duration::ZERO).unwrap();
        assert!(matches!(
            store.push(&noise(2), Duration::from_millis(67)),
            Err(AnimationError::Budget { .. })
        ));
        store.pop().unwrap();
        store.push(&noise(2), Duration::from_millis(133)).unwrap();
        let frame = store.pop().unwrap();
        assert_eq!(frame.area, Area::whole(64, 64));
        assert_eq!(frame.at, Duration::from_millis(133));
    }

    #[test]
    fn a_damaged_frame_cannot_be_read_back() {
        let mut store = unlimited(4, 4);
        store
            .push(&image(4, 4, |x, _| [x as u8 * 60, 0, 0]), Duration::ZERO)
            .unwrap();
        let mut frame = store.pop().unwrap();
        assert!(frame.cut().is_some());
        frame.data = frame.data[..frame.data.len() - 1].into();
        assert!(frame.cut().is_none());
    }

    #[test]
    fn a_frame_of_another_size_is_refused() {
        let mut store = unlimited(4, 4);
        assert!(matches!(
            store.push(&image(5, 4, |_, _| BLACK), Duration::ZERO),
            Err(AnimationError::FrameSize { .. })
        ));
        assert!(store.pop().is_none());
    }

    #[test]
    fn sizes_a_gif_cannot_hold_are_refused() {
        for (width, height) in [(0, 10), (10, 0), (70_000, 10)] {
            assert!(matches!(
                FrameStore::new(width, height, usize::MAX),
                Err(AnimationError::Size { .. })
            ));
        }
    }
}
