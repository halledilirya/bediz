mod change;
mod dither;
mod frame;
mod palette;
mod timing;

use std::io::Write;
use std::time::Duration;

use thiserror::Error;

use crate::image::Image;
use change::{Area, Rgb};
use frame::IndexedFrame;
use timing::Timing;

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
    #[error("the frame's colours could not be reduced to a palette")]
    Palette,
    #[error("an animation needs at least one frame")]
    NoFrames,
    #[error(transparent)]
    Gif(#[from] gif::EncodingError),
}

pub struct GifEncoder<W: Write> {
    encoder: gif::Encoder<W>,
    width: u16,
    height: u16,
    timing: Timing,
    previous: Option<Vec<Rgb>>,
    pending: Option<(u32, IndexedFrame)>,
}

impl<W: Write> GifEncoder<W> {
    pub fn new(writer: W, width: u32, height: u32) -> Result<Self, AnimationError> {
        let (Ok(gif_width), Ok(gif_height)) = (u16::try_from(width), u16::try_from(height)) else {
            return Err(AnimationError::Size { width, height });
        };
        if gif_width == 0 || gif_height == 0 {
            return Err(AnimationError::Size { width, height });
        }
        let mut encoder = gif::Encoder::new(writer, gif_width, gif_height, &[])?;
        encoder.set_repeat(gif::Repeat::Infinite)?;
        Ok(Self {
            encoder,
            width: gif_width,
            height: gif_height,
            timing: Timing::default(),
            previous: None,
            pending: None,
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
        let current: Vec<Rgb> = image
            .bgra()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|[blue, green, red, _]| [*red, *green, *blue])
            .collect();
        let area = match &self.previous {
            Some(previous) => change::changed_area(previous, &current, self.width),
            None => Some(Area::whole(self.width, self.height)),
        };
        let Some(area) = area else {
            return Ok(());
        };
        let cut = change::cut(self.previous.as_deref(), &current, self.width, area);
        let frame = IndexedFrame::new(&cut, area).ok_or(AnimationError::Palette)?;
        let start = self.timing.start(at);
        self.write_pending(start)?;
        self.pending = Some((start, frame));
        self.previous = Some(current);
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

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
        let first = frames[0].0;
        let mut encoder = GifEncoder::new(Vec::new(), first.width(), first.height()).unwrap();
        for (frame, millis) in frames {
            encoder.push(frame, Duration::from_millis(*millis)).unwrap();
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
    fn a_frame_of_another_size_is_refused() {
        let mut encoder = GifEncoder::new(Vec::new(), 4, 4).unwrap();
        let result = encoder.push(&image(5, 4, |_, _| BLACK), Duration::ZERO);
        assert!(matches!(result, Err(AnimationError::FrameSize { .. })));
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
