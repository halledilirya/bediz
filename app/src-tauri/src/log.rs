use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

const LIMIT_BYTES: u64 = 1_048_576;

pub struct Log {
    directory: PathBuf,
    current: PathBuf,
    previous: PathBuf,
    started: Instant,
}

impl Log {
    pub fn open(directory: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&directory)?;
        Ok(Self {
            current: directory.join("bediz.log"),
            previous: directory.join("bediz.old.log"),
            directory,
            started: Instant::now(),
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn write(&self, line: &str) {
        if fs::metadata(&self.current).is_ok_and(|metadata| metadata.len() > LIMIT_BYTES) {
            // Rotation replaces the older log on purpose; the no-overwrite rule is for captures.
            let _ = fs::rename(&self.current, &self.previous);
        }
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.current)
        {
            let _ = writeln!(
                file,
                "{:>10.3} {line}",
                self.started.elapsed().as_secs_f64()
            );
        }
    }
}
