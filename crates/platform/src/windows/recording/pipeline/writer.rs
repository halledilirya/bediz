use std::collections::BTreeMap;
use std::ffi::c_void;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use bediz_core::animation::GifEncoder;
use bediz_core::delivery::SaveError;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_NORMAL};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::pace::{Pace, Span};
use super::{Ended, INTERNAL_ERROR, Phase, Prepared, Shared, save_error, set_priority};
use crate::event::SavedGif;
use crate::windows::runtime::WM_RECORDING;
use crate::windows::save;

type Encoder = GifEncoder<BufWriter<File>>;

pub(super) struct Writing {
    pub(super) receiver: Receiver<Prepared>,
    pub(super) encoder: Encoder,
    pub(super) shared: Arc<Shared>,
    pub(super) temporary: PathBuf,
    pub(super) folder: PathBuf,
    pub(super) stem: String,
    pub(super) size: (u32, u32),
    pub(super) threads: usize,
    pub(super) ended: Arc<Mutex<Option<Ended>>>,
    pub(super) window: isize,
}

impl Writing {
    pub(super) fn run(self) {
        let Self {
            receiver,
            encoder,
            shared,
            temporary,
            folder,
            stem,
            size,
            threads,
            ended,
            window,
        } = self;
        set_priority(THREAD_PRIORITY_BELOW_NORMAL);
        let mut pace = Pace::new(threads, size);
        let written = catch_unwind(AssertUnwindSafe(
            || -> Result<Option<SavedGif>, SaveError> {
                let Some((encoder, end)) =
                    write(receiver, encoder, &shared, &mut pace, &|| tell(window))?
                else {
                    return Ok(None);
                };
                let closing = Instant::now();
                let (path, bytes) = close(encoder, end, &temporary, &folder, &stem)?;
                pace.closed_in(closing.elapsed());
                Ok(Some(SavedGif {
                    path,
                    bytes,
                    length: end,
                    width: size.0,
                    height: size.1,
                }))
            },
        ))
        .unwrap_or_else(|_| Err(SaveError::Unexpected(INTERNAL_ERROR.to_owned())));
        let outcome = match written {
            Ok(Some(saved)) => Ended::Saved(saved, pace.to_string()),
            Ok(None) => Ended::Cancelled,
            Err(_) if shared.phase() == Phase::Cancelled => Ended::Cancelled,
            Err(error) => {
                shared.fail();
                Ended::Failed(error)
            }
        };
        if !matches!(outcome, Ended::Saved(..)) {
            let _ = fs::remove_file(&temporary);
        }
        *ended.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
        tell(window);
    }
}

fn tell(window: isize) {
    let window = HWND(window as *mut c_void);
    let _ = unsafe { PostMessageW(Some(window), WM_RECORDING, WPARAM(0), LPARAM(0)) };
}

fn write<W: Write>(
    receiver: Receiver<Prepared>,
    mut encoder: GifEncoder<W>,
    shared: &Shared,
    pace: &mut Pace,
    tell: &dyn Fn(),
) -> Result<Option<(GifEncoder<W>, Duration)>, SaveError> {
    let mut waiting = BTreeMap::new();
    let mut wanted = 0u64;
    let mut raised = false;
    let mut told = None;
    for prepared in receiver {
        if !raised && matches!(shared.phase(), Phase::Finishing(..)) {
            set_priority(THREAD_PRIORITY_NORMAL);
            raised = true;
        }
        waiting.insert(prepared.number, prepared);
        let before = wanted;
        while let Some(prepared) = waiting.remove(&wanted) {
            let start = Instant::now();
            let written = prepared
                .frame
                .and_then(|frame| encoder.push(frame).map_err(|error| save_error(&error)));
            if let Err(error) = written {
                shared.fail();
                return Err(error);
            }
            let writing = Span {
                start,
                end: Instant::now(),
            };
            pace.wrote(
                prepared.preparing,
                writing,
                prepared.changed,
                shared.stopped(),
            );
            wanted += 1;
        }
        if wanted != before {
            shared.written(wanted);
            let percent = shared.written_percent();
            if percent.is_some() && percent != told {
                told = percent;
                tell();
            }
        }
    }
    pace.ended(shared.stopped(), Instant::now());
    match shared.phase() {
        Phase::Cancelled => Ok(None),
        Phase::Finishing(end, _) if waiting.is_empty() => Ok(Some((encoder, end))),
        _ => Err(SaveError::Unexpected(
            "the recording ended before all of its frames were written".to_owned(),
        )),
    }
}

fn close(
    encoder: Encoder,
    end: Duration,
    temporary: &Path,
    folder: &Path,
    stem: &str,
) -> Result<(PathBuf, u64), SaveError> {
    let writer = encoder.finish(end).map_err(|error| save_error(&error))?;
    let file = writer
        .into_inner()
        .map_err(|error| save::io_error(error.error()))?;
    file.sync_all().map_err(|error| save::io_error(&error))?;
    let bytes = file.metadata().map_or(0, |metadata| metadata.len());
    drop(file);
    save::finish_recording(temporary, folder, stem).map(|path| (path, bytes))
}

#[cfg(test)]
mod tests {
    use std::sync::{Condvar, mpsc};

    use bediz_core::animation::{FrameStore, PreparedFrame};
    use bediz_core::image::Image;

    use super::super::State;
    use super::*;

    #[test]
    fn time_after_the_stop_counts_from_the_stop_and_not_from_the_next_frame() {
        let now = Instant::now();
        let stop = now.checked_sub(Duration::from_secs(1)).unwrap();
        let mut store = FrameStore::new(4, 4, usize::MAX).unwrap();
        store
            .push(
                &Image::from_bgra(4, 4, vec![9; 64]).unwrap(),
                Duration::ZERO,
            )
            .unwrap();
        let frame = store.pop().unwrap();
        let shared = Shared {
            state: Mutex::new(State {
                store,
                handed_out: 1,
                written: 0,
                window: 2,
                phase: Phase::Finishing(Duration::from_secs(1), stop),
            }),
            frames: Condvar::new(),
        };
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Prepared {
                number: 0,
                frame: Ok(PreparedFrame::new(&frame).unwrap()),
                preparing: Span {
                    start: stop.checked_sub(Duration::from_millis(500)).unwrap(),
                    end: now,
                },
                changed: frame.changed_area(),
            })
            .unwrap();
        drop(sender);
        let encoder = GifEncoder::new(Vec::new(), 4, 4).unwrap();
        let mut pace = Pace::new(1, (4, 4));
        let written = std::thread::scope(|scope| {
            scope
                .spawn(|| write(receiver, encoder, &shared, &mut pace, &|| {}))
                .join()
                .unwrap()
        });
        assert!(matches!(written, Ok(Some((_, end))) if end == Duration::from_secs(1)));
        assert_eq!(pace.frames, 1);
        assert_eq!(pace.after_stop, 1);
        assert!(pace.since_stop >= Duration::from_secs(1));
        assert_eq!(pace.preparing, Duration::from_millis(1500));
        assert_eq!(pace.preparing_since, Duration::from_secs(1));
        assert!(pace.writing_since <= pace.since_stop);
    }
}
