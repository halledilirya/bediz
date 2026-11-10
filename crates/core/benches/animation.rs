use std::collections::BTreeMap;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use bediz_core::animation::{FrameStore, GifEncoder, PreparedFrame, StoredFrame};
use bediz_core::image::Image;

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
const FRAMES: u32 = 30;
const FRAME_TIME: Duration = Duration::from_nanos(1_000_000_000 / 15);
const THREADS: [usize; 2] = [2, 3];

#[derive(Clone, Copy)]
enum Clip {
    Interface,
    Scrolling,
    Video,
}

impl Clip {
    fn name(self) -> &'static str {
        match self {
            Self::Interface => "A window dragged over a desktop",
            Self::Scrolling => "A page of text scrolling",
            Self::Video => "Moving gradients with noise",
        }
    }

    fn frame(self, frame: u32) -> Result<Image, Box<dyn Error>> {
        let mut bgra = Vec::with_capacity(WIDTH as usize * HEIGHT as usize * 4);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let [red, green, blue] = match self {
                    Self::Interface => interface(frame, x, y),
                    Self::Scrolling => scrolling(frame, x, y),
                    Self::Video => video(frame, x, y),
                };
                bgra.extend_from_slice(&[blue, green, red, u8::MAX]);
            }
        }
        Ok(Image::from_bgra(WIDTH, HEIGHT, bgra)?)
    }
}

#[derive(Default)]
struct Timings {
    storing: Duration,
    preparing: Duration,
    writing: Duration,
    slowest: Duration,
    stored: usize,
    gif: Vec<u8>,
}

fn main() -> Result<(), Box<dyn Error>> {
    println!(
        "{FRAMES} frames of {WIDTH}x{HEIGHT} a clip; at 15 fps each frame has {:.1} ms; {} threads available",
        millis(FRAME_TIME),
        thread::available_parallelism()?
    );
    let raw = WIDTH as usize * HEIGHT as usize * 4 * FRAMES as usize;
    for clip in [Clip::Interface, Clip::Scrolling, Clip::Video] {
        let (timings, frames) = measure(clip)?;
        println!();
        println!("{}", clip.name());
        println!(
            "  store:  {:.1} ms a frame, {:.1} MB kept of {:.1} MB raw",
            per_frame(timings.storing),
            megabytes(timings.stored),
            megabytes(raw)
        );
        println!(
            "  encode: {:.1} ms a frame, the slowest frame {:.1} ms",
            per_frame(timings.preparing + timings.writing),
            millis(timings.slowest)
        );
        println!(
            "    preparing (reading back, palette, dithering) {:.1} ms, writing (LZW) {:.1} ms",
            per_frame(timings.preparing),
            per_frame(timings.writing)
        );
        for threads in THREADS {
            let (took, gif) = in_parallel(&frames, threads)?;
            if gif != timings.gif {
                return Err(format!("preparing on {threads} threads made another file").into());
            }
            println!(
                "    preparing on {threads} threads: {:.1} ms a frame",
                per_frame(took)
            );
        }
        println!("  GIF:    {:.1} MB", megabytes(timings.gif.len()));
    }
    Ok(())
}

fn measure(clip: Clip) -> Result<(Timings, Vec<StoredFrame>), Box<dyn Error>> {
    let mut store = FrameStore::new(WIDTH, HEIGHT, usize::MAX)?;
    let mut encoder = GifEncoder::new(Vec::new(), WIDTH, HEIGHT)?;
    let mut timings = Timings::default();
    let mut frames = Vec::new();
    for n in 0..FRAMES {
        let image = clip.frame(n)?;
        let started = Instant::now();
        store.push(&image, FRAME_TIME * n)?;
        timings.storing += started.elapsed();
        timings.stored += store.used();
        while let Some(frame) = store.pop() {
            let started = Instant::now();
            let prepared = PreparedFrame::new(&frame)?;
            let preparing = started.elapsed();
            encoder.push(prepared)?;
            let took = started.elapsed();
            timings.preparing += preparing;
            timings.writing += took.saturating_sub(preparing);
            timings.slowest = timings.slowest.max(took);
            frames.push(frame);
        }
    }
    let started = Instant::now();
    timings.gif = encoder.finish(FRAME_TIME * FRAMES)?;
    timings.writing += started.elapsed();
    Ok((timings, frames))
}

fn in_parallel(
    frames: &[StoredFrame],
    threads: usize,
) -> Result<(Duration, Vec<u8>), Box<dyn Error>> {
    let next = AtomicUsize::new(0);
    let started = Instant::now();
    let gif = thread::scope(|scope| -> Result<Vec<u8>, Box<dyn Error>> {
        let (sender, receiver) = mpsc::channel();
        for _ in 0..threads {
            let sender = sender.clone();
            let next = &next;
            scope.spawn(move || {
                loop {
                    let n = next.fetch_add(1, Ordering::Relaxed);
                    let Some(frame) = frames.get(n) else {
                        break;
                    };
                    if sender.send((n, PreparedFrame::new(frame))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        let mut encoder = GifEncoder::new(Vec::new(), WIDTH, HEIGHT)?;
        let mut waiting = BTreeMap::new();
        let mut wanted = 0;
        for (n, prepared) in receiver {
            waiting.insert(n, prepared?);
            while let Some(prepared) = waiting.remove(&wanted) {
                encoder.push(prepared)?;
                wanted += 1;
            }
        }
        Ok(encoder.finish(FRAME_TIME * FRAMES)?)
    })?;
    Ok((started.elapsed(), gif))
}

fn interface(frame: u32, x: u32, y: u32) -> [u8; 3] {
    let left = 80 + frame * 12;
    let top = 120;
    if (left..left + 480).contains(&x) && (top..top + 320).contains(&y) {
        let (inside_x, inside_y) = (x - left, y - top);
        if inside_y < 32 {
            return [32, 32, 40];
        }
        if inside_y > 48 && inside_y % 20 < 10 && (12..460).contains(&inside_x) {
            return glyph(inside_x, inside_y / 20);
        }
        return [255, 255, 255];
    }
    if y >= HEIGHT - 48 {
        return [28, 28, 36];
    }
    if (24..72).contains(&x) && y % 96 < 48 && y < HEIGHT - 96 {
        return [0, 120, 215];
    }
    [243, 243, 243]
}

fn scrolling(frame: u32, x: u32, y: u32) -> [u8; 3] {
    let page_y = y + frame * 6;
    let line = page_y / 24;
    if page_y % 24 < 14 && (40..WIDTH - 40).contains(&x) {
        if line % 7 == 3 && glyph(x, line) != [255, 255, 255] {
            return [20, 90, 200];
        }
        return glyph(x, line);
    }
    [255, 255, 255]
}

fn glyph(x: u32, line: u32) -> [u8; 3] {
    if (x / 8 + line * 5).is_multiple_of(9) {
        return [255, 255, 255];
    }
    match x % 8 {
        0 | 1 => [30, 30, 30],
        2 => [90, 90, 90],
        3 | 4 => [30, 30, 30],
        5 => [160, 160, 160],
        _ => [255, 255, 255],
    }
}

fn video(frame: u32, x: u32, y: u32) -> [u8; 3] {
    let noise = (mix(x, y, frame) % 16) as u8;
    [
        wave(x + frame * 9).saturating_add(noise),
        wave(y * 2 + frame * 5).saturating_add(noise / 2),
        wave(x + y + frame * 3).saturating_sub(noise),
    ]
}

fn wave(value: u32) -> u8 {
    let value = value % 510;
    if value < 255 {
        value as u8
    } else {
        (510 - value) as u8
    }
}

fn mix(x: u32, y: u32, frame: u32) -> u32 {
    let mut value =
        x.wrapping_mul(0x9e37_79b1) ^ y.wrapping_mul(0x85eb_ca77) ^ frame.wrapping_mul(0xc2b2_ae3d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x2c1b_3c6d);
    value ^ (value >> 12)
}

fn per_frame(total: Duration) -> f64 {
    millis(total) / f64::from(FRAMES)
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn megabytes(bytes: usize) -> f64 {
    bytes as f64 / 1_000_000.0
}
