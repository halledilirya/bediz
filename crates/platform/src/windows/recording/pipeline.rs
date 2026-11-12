mod pace;
mod writer;

use std::fs::{self, File};
use std::io::BufWriter;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{Builder, JoinHandle};
use std::time::{Duration, Instant};

use bediz_core::animation::{AnimationError, FrameStore, GifEncoder, PreparedFrame};
use bediz_core::delivery::SaveError;
use bediz_core::image::Image;
use bediz_core::recording;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY, THREAD_PRIORITY_BELOW_NORMAL,
    THREAD_PRIORITY_NORMAL,
};

use crate::event::SavedGif;
use crate::windows::save;
use pace::Span;
use writer::Writing;

const INTERNAL_ERROR: &str = "an internal error was caught while writing the GIF";
const FRAMES_PER_THREAD: u64 = 2;

pub struct Output {
    pub file: File,
    pub temporary: PathBuf,
    pub folder: PathBuf,
    pub stem: String,
}

pub enum Ended {
    Saved(SavedGif, String),
    Failed(SaveError),
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Recording,
    Finishing(Duration, Instant),
    Cancelled,
    Failed,
}

struct State {
    store: FrameStore,
    handed_out: u64,
    written: u64,
    window: u64,
    phase: Phase,
}

impl State {
    fn has_room(&self) -> bool {
        self.handed_out.saturating_sub(self.written) < self.window
    }
}

struct Shared {
    state: Mutex<State>,
    frames: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn phase(&self) -> Phase {
        self.lock().phase
    }

    fn change(&self, change: impl FnOnce(Phase) -> Phase) {
        let mut state = self.lock();
        state.phase = change(state.phase);
        drop(state);
        self.frames.notify_all();
    }

    // Taken under the lock, so a frame the writer finishes after this time always sees it.
    fn finish(&self, end: Duration) {
        self.change(|phase| match phase {
            Phase::Recording => Phase::Finishing(end, Instant::now()),
            other => other,
        });
    }

    fn stopped(&self) -> Option<Instant> {
        match self.phase() {
            Phase::Finishing(_, at) => Some(at),
            _ => None,
        }
    }

    fn cancel(&self) {
        self.change(|phase| match phase {
            Phase::Recording | Phase::Finishing(..) => Phase::Cancelled,
            other => other,
        });
    }

    fn fail(&self) {
        self.change(|phase| match phase {
            Phase::Cancelled => Phase::Cancelled,
            _ => Phase::Failed,
        });
    }

    fn written(&self, count: u64) {
        self.lock().written = count;
        self.frames.notify_all();
    }

    fn written_share(&self) -> Option<(u64, u64)> {
        let state = self.lock();
        let Phase::Finishing(..) = state.phase else {
            return None;
        };
        let waiting = u64::try_from(state.store.waiting()).unwrap_or(u64::MAX);
        Some((state.written, state.handed_out.saturating_add(waiting)))
    }

    fn written_percent(&self) -> Option<u8> {
        self.written_share()
            .map(|(written, total)| recording::written_percent(written, total))
    }
}

struct Prepared {
    number: u64,
    frame: Result<PreparedFrame, SaveError>,
    preparing: Span,
    changed: u64,
}

pub struct Pipeline {
    shared: Arc<Shared>,
    ended: Arc<Mutex<Option<Ended>>>,
    threads: Vec<JoinHandle<()>>,
}

impl Pipeline {
    pub fn start(
        width: u32,
        height: u32,
        budget: usize,
        preparing: usize,
        output: Output,
        window: HWND,
    ) -> Result<Self, SaveError> {
        let Output {
            file,
            temporary,
            folder,
            stem,
        } = output;
        let setup = FrameStore::new(width, height, budget).and_then(|store| {
            GifEncoder::new(BufWriter::new(file), width, height).map(|encoder| (store, encoder))
        });
        let (store, encoder) = match setup {
            Ok(setup) => setup,
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(save_error(&error));
            }
        };
        let preparing = preparing.max(1);
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                store,
                handed_out: 0,
                written: 0,
                window: in_flight(preparing),
                phase: Phase::Recording,
            }),
            frames: Condvar::new(),
        });
        let ended = Arc::new(Mutex::new(None));
        let (sender, receiver) = mpsc::channel::<Prepared>();
        let writing = Writing {
            receiver,
            encoder,
            shared: Arc::clone(&shared),
            temporary: temporary.clone(),
            folder,
            stem,
            size: (width, height),
            threads: preparing,
            ended: Arc::clone(&ended),
            window: window.0 as isize,
        };
        let writer = match Builder::new()
            .name("bediz-gif-write".to_owned())
            .spawn(move || writing.run())
        {
            Ok(writer) => writer,
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(SaveError::Unexpected(error.to_string()));
            }
        };
        let mut pipeline = Self {
            shared,
            ended,
            threads: vec![writer],
        };
        for number in 0..preparing {
            let shared = Arc::clone(&pipeline.shared);
            let frames = sender.clone();
            let rises = recording::raised_after_stop(number);
            let spawned = Builder::new()
                .name(format!("bediz-gif-prepare-{number}"))
                .spawn(move || prepare(&shared, &frames, rises));
            match spawned {
                Ok(thread) => pipeline.threads.push(thread),
                Err(error) => {
                    drop(sender);
                    pipeline.cancel();
                    pipeline.join();
                    return Err(SaveError::Unexpected(error.to_string()));
                }
            }
        }
        drop(sender);
        Ok(pipeline)
    }

    pub fn push(&self, image: &Image, at: Duration) -> Result<(), AnimationError> {
        let mut state = self.shared.lock();
        if state.phase != Phase::Recording {
            return Ok(());
        }
        state.store.push(image, at)?;
        drop(state);
        self.shared.frames.notify_one();
        Ok(())
    }

    pub fn failed(&self) -> bool {
        self.shared.phase() == Phase::Failed
    }

    pub fn written_percent(&self) -> Option<u8> {
        self.shared.written_percent()
    }

    pub fn written_share(&self) -> Option<(u64, u64)> {
        self.shared.written_share()
    }

    pub fn finish(&self, end: Duration) {
        self.shared.finish(end);
    }

    pub fn cancel(&self) {
        self.shared.cancel();
    }

    pub fn take_ended(&self) -> Option<Ended> {
        self.ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    pub fn wait(mut self) -> Ended {
        self.join();
        self.take_ended().unwrap_or_else(|| {
            Ended::Failed(SaveError::Unexpected(
                "the GIF's threads ended without an answer".to_owned(),
            ))
        })
    }

    fn join(&mut self) {
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        if self.shared.phase() == Phase::Recording {
            self.cancel();
        }
        self.join();
    }
}

fn prepare(shared: &Shared, sender: &Sender<Prepared>, rises: bool) {
    set_priority(THREAD_PRIORITY_BELOW_NORMAL);
    let mut rise = rises;
    loop {
        let (number, frame, finishing) = {
            let mut state = shared.lock();
            loop {
                if matches!(state.phase, Phase::Cancelled | Phase::Failed) {
                    return;
                }
                if state.has_room() {
                    if let Some(frame) = state.store.pop() {
                        let number = state.handed_out;
                        state.handed_out += 1;
                        break (number, frame, state.phase != Phase::Recording);
                    }
                    if state.phase != Phase::Recording {
                        return;
                    }
                }
                state = shared
                    .frames
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        if finishing && rise {
            set_priority(THREAD_PRIORITY_NORMAL);
            rise = false;
        }
        let start = Instant::now();
        let prepared = catch_unwind(AssertUnwindSafe(|| PreparedFrame::new(&frame)))
            .map_err(|_| SaveError::Unexpected(INTERNAL_ERROR.to_owned()))
            .and_then(|prepared| prepared.map_err(|error| save_error(&error)));
        let prepared = Prepared {
            number,
            frame: prepared,
            preparing: Span {
                start,
                end: Instant::now(),
            },
            changed: frame.changed_area(),
        };
        if sender.send(prepared).is_err() {
            return;
        }
    }
}

fn in_flight(preparing: usize) -> u64 {
    u64::try_from(preparing)
        .unwrap_or(u64::MAX)
        .saturating_mul(FRAMES_PER_THREAD)
}

fn save_error(error: &AnimationError) -> SaveError {
    match error.io() {
        Some(error) => save::io_error(error),
        None => SaveError::Unexpected(error.to_string()),
    }
}

fn set_priority(priority: THREAD_PRIORITY) {
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), priority) };
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;

    use super::*;

    const WAIT: Duration = Duration::from_secs(10);
    const NOTHING_MORE: Duration = Duration::from_millis(200);

    fn shared(frames: u8, window: u64) -> Arc<Shared> {
        let mut store = FrameStore::new(4, 4, usize::MAX).unwrap();
        for value in 0..frames {
            let image = Image::from_bgra(4, 4, vec![value; 64]).unwrap();
            store
                .push(&image, Duration::from_millis(u64::from(value) * 67))
                .unwrap();
        }
        Arc::new(Shared {
            state: Mutex::new(State {
                store,
                handed_out: 0,
                written: 0,
                window,
                phase: Phase::Recording,
            }),
            frames: Condvar::new(),
        })
    }

    fn start_preparing(shared: &Arc<Shared>) -> (Receiver<Prepared>, Receiver<()>) {
        let (sender, frames) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let shared = Arc::clone(shared);
        std::thread::spawn(move || {
            prepare(&shared, &sender, true);
            let _ = finished.send(());
        });
        (frames, done)
    }

    #[test]
    fn frames_stay_in_the_store_while_the_writer_falls_behind() {
        let shared = shared(6, 2);
        let (frames, done) = start_preparing(&shared);
        assert_eq!(frames.recv_timeout(WAIT).unwrap().number, 0);
        assert_eq!(frames.recv_timeout(WAIT).unwrap().number, 1);
        assert!(frames.recv_timeout(NOTHING_MORE).is_err());
        assert_eq!(shared.lock().handed_out, 2);
        shared.written(1);
        assert_eq!(frames.recv_timeout(WAIT).unwrap().number, 2);
        assert!(frames.recv_timeout(NOTHING_MORE).is_err());
        shared.cancel();
        done.recv_timeout(WAIT).unwrap();
        assert_eq!(shared.lock().handed_out, 3);
    }

    #[test]
    fn a_finishing_recording_hands_out_every_frame_as_the_writer_catches_up() {
        let shared = shared(3, 1);
        shared.finish(Duration::from_secs(1));
        let (frames, done) = start_preparing(&shared);
        for number in 0..3 {
            let prepared = frames.recv_timeout(WAIT).unwrap();
            assert_eq!(prepared.number, number);
            assert!(prepared.frame.is_ok());
            assert_eq!(prepared.changed, 16);
            shared.written(number + 1);
        }
        done.recv_timeout(WAIT).unwrap();
    }

    #[test]
    fn the_share_written_counts_frames_taken_and_still_waiting_once_finishing() {
        let shared = shared(4, 2);
        assert_eq!(shared.written_percent(), None);
        shared.finish(Duration::from_secs(1));
        assert_eq!(shared.written_percent(), Some(0));
        let mut state = shared.lock();
        state.store.pop().unwrap();
        state.store.pop().unwrap();
        state.handed_out = 2;
        drop(state);
        shared.written(1);
        assert_eq!(shared.written_share(), Some((1, 4)));
        assert_eq!(shared.written_percent(), Some(25));
        shared.cancel();
        assert_eq!(shared.written_share(), None);
        assert_eq!(shared.written_percent(), None);
    }

    #[test]
    fn the_window_grows_with_the_preparing_threads() {
        assert_eq!(in_flight(1), 2);
        assert_eq!(in_flight(3), 6);
        assert_eq!(in_flight(6), 12);
    }
}
