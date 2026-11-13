mod change;
mod codec;
mod dither;
mod frame;
mod nearest;
mod palette;
mod store;
mod timing;

use std::io::Write;
use std::time::Duration;

use thiserror::Error;

use frame::IndexedFrame;
use timing::Timing;

pub use store::{FrameStore, StoredFrame};

#[derive(Debug, Error)]
pub enum AnimationError {
    #[error("a GIF needs a width and a height from 1 to 65535, got {width}x{height}")]
    Size { width: u32, height: u32 },
    #[error("a {actual_width}x{actual_height} frame does not fit a {width}x{height} animation")]
    FrameSize {
        width: u16,
        height: u16,
        actual_width: u32,
        actual_height: u32,
    },
    #[error("the recording reached its memory budget of {budget} bytes")]
    Budget { budget: usize },
    #[error("a stored frame could not be read back")]
    Unreadable,
    #[error("the frame's colours could not be reduced to a palette")]
    Palette,
    #[error("an animation needs at least one frame")]
    NoFrames,
    #[error(transparent)]
    Gif(#[from] gif::EncodingError),
}

impl AnimationError {
    pub fn io(&self) -> Option<&std::io::Error> {
        match self {
            Self::Gif(gif::EncodingError::Io(error)) => Some(error),
            _ => None,
        }
    }
}

pub struct PreparedFrame {
    at: Duration,
    canvas: (u16, u16),
    indexed: IndexedFrame,
}

impl PreparedFrame {
    pub fn new(frame: &StoredFrame) -> Result<Self, AnimationError> {
        let cut = frame.cut().ok_or(AnimationError::Unreadable)?;
        let indexed = IndexedFrame::new(&cut, frame.area).ok_or(AnimationError::Palette)?;
        Ok(Self {
            at: frame.at,
            canvas: frame.canvas,
            indexed,
        })
    }
}

pub struct GifEncoder<W: Write> {
    encoder: gif::Encoder<W>,
    width: u16,
    height: u16,
    timing: Timing,
    pending: Option<(u32, IndexedFrame)>,
}

impl<W: Write> GifEncoder<W> {
    pub fn new(writer: W, width: u32, height: u32) -> Result<Self, AnimationError> {
        let (width, height) = gif_size(width, height)?;
        let mut encoder = gif::Encoder::new(writer, width, height, &[])?;
        encoder.set_repeat(gif::Repeat::Infinite)?;
        Ok(Self {
            encoder,
            width,
            height,
            timing: Timing::default(),
            pending: None,
        })
    }

    pub fn push(&mut self, frame: PreparedFrame) -> Result<(), AnimationError> {
        let (width, height) = frame.canvas;
        if (width, height) != (self.width, self.height) {
            return Err(AnimationError::FrameSize {
                width: self.width,
                height: self.height,
                actual_width: u32::from(width),
                actual_height: u32::from(height),
            });
        }
        let start = self.timing.start(frame.at);
        self.write_pending(start)?;
        self.pending = Some((start, frame.indexed));
        Ok(())
    }

    pub fn finish(mut self, end: Duration) -> Result<W, AnimationError> {
        if self.pending.is_none() {
            return Err(AnimationError::NoFrames);
        }
        let end = self.timing.end(end);
        self.write_pending(end)?;
        Ok(self.encoder.into_inner()?)
    }

    fn write_pending(&mut self, end: u32) -> Result<(), AnimationError> {
        let Some((start, frame)) = self.pending.take() else {
            return Ok(());
        };
        let delays = timing::delays(start, end);
        let Some((first, rest)) = delays.split_first() else {
            return Ok(());
        };
        self.encoder.write_frame(&frame.into_gif(*first))?;
        for delay in rest {
            self.encoder.write_frame(&frame::still(*delay))?;
        }
        Ok(())
    }
}

fn gif_size(width: u32, height: u32) -> Result<(u16, u16), AnimationError> {
    match (u16::try_from(width), u16::try_from(height)) {
        (Ok(gif_width), Ok(gif_height)) if gif_width > 0 && gif_height > 0 => {
            Ok((gif_width, gif_height))
        }
        _ => Err(AnimationError::Size { width, height }),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::image::Image;

    const BLACK: [u8; 3] = [0, 0, 0];
    const RED: [u8; 3] = [255, 0, 0];

    struct Decoded {
        delay: u16,
        area: (u16, u16, u16, u16),
        transparent: Option<u8>,
        alpha: Vec<u8>,
        canvas: Vec<[u8; 3]>,
    }

    fn image(width: u32, height: u32, colour: impl Fn(u32, u32) -> [u8; 3]) -> Image {
        let mut bgra = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let [red, green, blue] = colour(x, y);
                bgra.extend_from_slice(&[blue, green, red, 255]);
            }
        }
        Image::from_bgra(width, height, bgra).unwrap()
    }

    fn rgb(image: &Image) -> Vec<[u8; 3]> {
        image
            .bgra()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|[blue, green, red, _]| [*red, *green, *blue])
            .collect()
    }

    fn encode(frames: &[(&Image, u64)], end: u64) -> Vec<u8> {
        let (width, height) = (frames[0].0.width(), frames[0].0.height());
        let mut store = FrameStore::new(width, height, usize::MAX).unwrap();
        let mut encoder = GifEncoder::new(Vec::new(), width, height).unwrap();
        for (frame, millis) in frames {
            store.push(frame, Duration::from_millis(*millis)).unwrap();
            while let Some(stored) = store.pop() {
                encoder.push(PreparedFrame::new(&stored).unwrap()).unwrap();
            }
        }
        encoder.finish(Duration::from_millis(end)).unwrap()
    }

    fn decode(file: &[u8]) -> (gif::Repeat, Vec<Decoded>) {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(Cursor::new(file)).unwrap();
        let width = usize::from(decoder.width());
        let mut canvas = vec![BLACK; width * usize::from(decoder.height())];
        let mut frames = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            let frame_width = usize::from(frame.width);
            let mut alpha = Vec::new();
            for (i, [red, green, blue, opacity]) in
                frame.buffer.as_chunks::<4>().0.iter().enumerate()
            {
                alpha.push(*opacity);
                if *opacity != 0 {
                    let x = usize::from(frame.left) + i % frame_width;
                    let y = usize::from(frame.top) + i / frame_width;
                    canvas[y * width + x] = [*red, *green, *blue];
                }
            }
            frames.push(Decoded {
                delay: frame.delay,
                area: (frame.left, frame.top, frame.width, frame.height),
                transparent: frame.transparent,
                alpha,
                canvas: canvas.clone(),
            });
        }
        (decoder.repeat(), frames)
    }

    #[test]
    fn the_first_frame_covers_everything_and_is_opaque() {
        let first = image(4, 3, |x, y| if x > y { RED } else { BLACK });
        let (repeat, frames) = decode(&encode(&[(&first, 0)], 500));
        assert_eq!(repeat, gif::Repeat::Infinite);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].area, (0, 0, 4, 3));
        assert_eq!(frames[0].transparent, None);
        assert_eq!(frames[0].delay, 50);
        assert_eq!(frames[0].canvas, rgb(&first));
    }

    #[test]
    fn identical_frames_merge_into_a_longer_one() {
        let black = image(4, 4, |_, _| BLACK);
        let red = image(4, 4, |_, _| RED);
        let (_, frames) = decode(&encode(&[(&black, 0), (&black, 100), (&red, 200)], 300));
        let delays: Vec<u16> = frames.iter().map(|frame| frame.delay).collect();
        assert_eq!(delays, [20, 10]);
    }

    #[test]
    fn a_frame_holds_only_the_rectangle_that_changed() {
        let before = image(8, 6, |_, _| BLACK);
        let after = image(8, 6, |x, y| {
            if (3..5).contains(&x) && (2..4).contains(&y) {
                RED
            } else {
                BLACK
            }
        });
        let (_, frames) = decode(&encode(&[(&before, 0), (&after, 100)], 200));
        assert_eq!(frames[1].area, (3, 2, 2, 2));
        assert_eq!(frames[1].canvas, rgb(&after));
    }

    #[test]
    fn unchanged_pixels_inside_the_rectangle_stay_transparent() {
        let before = image(6, 5, |_, _| BLACK);
        let after = image(6, 5, |x, y| {
            if (x, y) == (1, 1) || (x, y) == (4, 3) {
                RED
            } else {
                BLACK
            }
        });
        let (_, frames) = decode(&encode(&[(&before, 0), (&after, 100)], 200));
        let second = &frames[1];
        assert_eq!(second.area, (1, 1, 4, 3));
        assert!(second.transparent.is_some());
        let opaque: Vec<usize> = second
            .alpha
            .iter()
            .enumerate()
            .filter(|(_, alpha)| **alpha != 0)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(opaque, [0, 11]);
        assert_eq!(second.canvas, rgb(&after));
    }

    #[test]
    fn many_colours_keep_the_average_of_every_block() {
        let gradient = image(64, 64, |x, y| {
            [(x * 4) as u8, (y * 4) as u8, ((x + y) * 2) as u8]
        });
        let (_, frames) = decode(&encode(&[(&gradient, 0)], 100));
        let source = rgb(&gradient);
        let shown = &frames[0].canvas;
        for block_y in 0..8 {
            for block_x in 0..8 {
                for channel in 0..3 {
                    let mut wanted = 0i32;
                    let mut got = 0i32;
                    for y in block_y * 8..block_y * 8 + 8 {
                        for x in block_x * 8..block_x * 8 + 8 {
                            wanted += i32::from(source[y * 64 + x][channel]);
                            got += i32::from(shown[y * 64 + x][channel]);
                        }
                    }
                    let off = (wanted - got).abs() / 64;
                    assert!(
                        off <= 8,
                        "block {block_x},{block_y} channel {channel} off by {off}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_small_change_over_many_colours_leaves_the_rest_alone() {
        let gradient = image(32, 32, |x, y| [(x * 8) as u8, (y * 8) as u8, 99]);
        let patched = image(32, 32, |x, y| {
            if (10..14).contains(&x) && (20..22).contains(&y) {
                RED
            } else {
                [(x * 8) as u8, (y * 8) as u8, 99]
            }
        });
        let (_, frames) = decode(&encode(&[(&gradient, 0), (&patched, 100)], 200));
        assert_eq!(frames[1].area, (10, 20, 4, 2));
        let before = &frames[0].canvas;
        let after = &frames[1].canvas;
        for (i, (then, now)) in before.iter().zip(after).enumerate() {
            let (x, y) = (i % 32, i / 32);
            if (10..14).contains(&x) && (20..22).contains(&y) {
                assert_eq!(*now, RED);
            } else {
                assert_eq!(now, then);
            }
        }
    }

    #[test]
    fn fifteen_frames_a_second_add_up_to_the_recording() {
        let frames: Vec<Image> = (0..16u8)
            .map(|n| image(2, 2, move |_, _| [n * 10, 0, 0]))
            .collect();
        let timed: Vec<(&Image, u64)> = frames
            .iter()
            .enumerate()
            .map(|(n, frame)| (frame, n as u64 * 1000 / 15))
            .collect();
        let (_, decoded) = decode(&encode(&timed, 1067));
        assert_eq!(decoded.len(), 16);
        assert!(
            decoded
                .iter()
                .all(|frame| frame.delay == 6 || frame.delay == 7)
        );
        let total: u32 = decoded.iter().map(|frame| u32::from(frame.delay)).sum();
        assert_eq!(total, 107);
    }

    #[test]
    fn a_frame_longer_than_one_gif_delay_keeps_its_whole_time() {
        let only = image(1, 1, |_, _| RED);
        let (_, frames) = decode(&encode(&[(&only, 0)], 700_000));
        let delays: Vec<u16> = frames.iter().map(|frame| frame.delay).collect();
        assert_eq!(delays, [65_535, 4_465]);
        assert!(frames.iter().all(|frame| frame.canvas == rgb(&only)));
    }

    #[test]
    fn a_change_after_a_long_still_starts_on_time() {
        let black = image(2, 2, |_, _| BLACK);
        let red = image(2, 2, |_, _| RED);
        let (_, frames) = decode(&encode(&[(&black, 0), (&red, 655_360)], 655_400));
        let delays: Vec<u16> = frames.iter().map(|frame| frame.delay).collect();
        assert_eq!(delays, [65_534, 2, 4]);
        assert_eq!(frames[1].transparent, Some(0));
        assert_eq!(frames[1].canvas, rgb(&black));
        assert_eq!(frames[2].canvas, rgb(&red));
    }

    #[test]
    fn frames_prepared_out_of_order_on_other_threads_make_the_same_file() {
        let frames: Vec<Image> = (0..6u32)
            .map(|n| {
                image(32, 32, move |x, y| {
                    [(x * 8) as u8, ((y + n) * 8) as u8, (n * 40) as u8]
                })
            })
            .collect();
        let timed: Vec<(&Image, u64)> = frames.iter().zip((0..).step_by(100)).collect();
        let expected = encode(&timed, 600);
        let mut store = FrameStore::new(32, 32, usize::MAX).unwrap();
        for (frame, millis) in &timed {
            store.push(frame, Duration::from_millis(*millis)).unwrap();
        }
        let stored: Vec<StoredFrame> = std::iter::from_fn(|| store.pop()).collect();
        let prepared: Vec<PreparedFrame> = std::thread::scope(|scope| {
            let handles: Vec<_> = stored
                .iter()
                .rev()
                .map(|frame| scope.spawn(move || PreparedFrame::new(frame).unwrap()))
                .collect();
            handles
                .into_iter()
                .rev()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        let mut encoder = GifEncoder::new(Vec::new(), 32, 32).unwrap();
        for frame in prepared {
            encoder.push(frame).unwrap();
        }
        assert_eq!(
            encoder.finish(Duration::from_millis(600)).unwrap(),
            expected
        );
    }

    #[test]
    fn a_large_flat_area_among_many_colours_comes_out_exact() {
        let background = [30, 30, 30];
        let busy = image(64, 64, |x, y| {
            if y < 16 {
                [(x * 4) as u8, (y * 16) as u8, ((x + y) * 3) as u8]
            } else {
                background
            }
        });
        let (_, frames) = decode(&encode(&[(&busy, 0)], 100));
        assert!(
            frames[0].canvas[16 * 64..]
                .iter()
                .all(|pixel| *pixel == background)
        );
    }

    #[test]
    fn sizes_a_gif_cannot_hold_are_refused() {
        assert!(matches!(
            GifEncoder::new(Vec::new(), 0, 10),
            Err(AnimationError::Size { .. })
        ));
        assert!(matches!(
            GifEncoder::new(Vec::new(), 70_000, 10),
            Err(AnimationError::Size { .. })
        ));
    }

    #[test]
    fn a_frame_from_a_recording_of_another_size_is_refused() {
        let mut store = FrameStore::new(5, 4, usize::MAX).unwrap();
        store
            .push(&image(5, 4, |_, _| BLACK), Duration::ZERO)
            .unwrap();
        let frame = PreparedFrame::new(&store.pop().unwrap()).unwrap();
        let mut encoder = GifEncoder::new(Vec::new(), 4, 4).unwrap();
        assert!(matches!(
            encoder.push(frame),
            Err(AnimationError::FrameSize {
                width: 4,
                height: 4,
                actual_width: 5,
                actual_height: 4
            })
        ));
    }

    #[test]
    fn a_failed_write_keeps_the_system_error() {
        let full = std::io::Error::from(std::io::ErrorKind::StorageFull);
        let error = AnimationError::from(gif::EncodingError::from(full));
        assert_eq!(
            error.io().map(std::io::Error::kind),
            Some(std::io::ErrorKind::StorageFull)
        );
        assert!(AnimationError::NoFrames.io().is_none());
    }

    #[test]
    fn an_animation_without_frames_is_refused() {
        let encoder = GifEncoder::new(Vec::new(), 4, 4).unwrap();
        assert!(matches!(
            encoder.finish(Duration::ZERO),
            Err(AnimationError::NoFrames)
        ));
    }
}
