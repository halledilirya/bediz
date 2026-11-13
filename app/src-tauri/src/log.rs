//! The log file: lines with their time since start, only warnings in a release, the profile path
//! hidden, one older file kept, and panics recorded.

use std::cell::Cell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

const LIMIT_BYTES: u64 = 1_048_576;
const PROFILE: &str = "%USERPROFILE%";

pub struct Log {
    directory: PathBuf,
    file: LogFile,
    header: String,
    written: Cell<bool>,
    verbose: bool,
    started: Instant,
}

#[derive(Clone)]
struct LogFile {
    current: PathBuf,
    previous: PathBuf,
    profile: Option<String>,
}

impl Log {
    pub fn open(directory: PathBuf, header: String) -> std::io::Result<Self> {
        fs::create_dir_all(&directory)?;
        Ok(Self {
            file: LogFile {
                current: directory.join("bediz.log"),
                previous: directory.join("bediz.old.log"),
                profile: std::env::var("USERPROFILE").ok(),
            },
            directory,
            header,
            written: Cell::new(false),
            // Development builds keep every line for the user's timing runs; a release keeps warnings.
            verbose: cfg!(debug_assertions),
            started: Instant::now(),
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn warn(&self, line: &str) {
        self.write(line);
    }

    pub fn info(&self, line: &str) {
        if self.verbose {
            self.write(line);
        }
    }

    pub fn catch_panics(&self) {
        let file = self.file.clone();
        let earlier = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            file.append(&format!("panic {}", info.to_string().replace('\n', " ")));
            earlier(info);
        }));
    }

    fn write(&self, line: &str) {
        let elapsed = self.started.elapsed().as_secs_f64();
        if !self.written.replace(true) {
            self.file
                .append(&format!("{elapsed:>10.3} {}", self.header));
        }
        self.file.append(&format!("{elapsed:>10.3} {line}"));
    }
}

impl LogFile {
    fn append(&self, line: &str) {
        if fs::metadata(&self.current).is_ok_and(|metadata| metadata.len() > LIMIT_BYTES) {
            // Rotation replaces the older log on purpose; the no-overwrite rule is for captures.
            let _ = fs::rename(&self.current, &self.previous);
        }
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.current)
        {
            let _ = writeln!(file, "{}", redacted(line, self.profile.as_deref()));
        }
    }
}

fn redacted(line: &str, profile: Option<&str>) -> String {
    let Some(profile) = profile.filter(|profile| !profile.is_empty()) else {
        return line.to_owned();
    };
    let haystack = line.to_ascii_lowercase();
    let mut kept = String::with_capacity(line.len());
    let mut from = 0;
    for (start, found) in haystack.match_indices(&profile.to_ascii_lowercase()) {
        kept.push_str(line.get(from..start).unwrap_or_default());
        kept.push_str(PROFILE);
        from = start + found.len();
    }
    kept.push_str(line.get(from..).unwrap_or_default());
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: Option<&str> = Some(r"C:\Users\Ada");

    #[test]
    fn the_profile_path_is_hidden_whatever_its_case() {
        assert_eq!(
            redacted(r"not saved to c:\users\ada\Pictures\x.png", ADA),
            r"not saved to %USERPROFILE%\Pictures\x.png"
        );
        assert_eq!(
            redacted(r"C:\Users\Ada\a and C:\USERS\ADA\b", ADA),
            r"%USERPROFILE%\a and %USERPROFILE%\b"
        );
    }

    #[test]
    fn other_lines_stay_as_they_are() {
        assert_eq!(redacted(r"D:\Captures\ş.png", ADA), r"D:\Captures\ş.png");
        assert_eq!(redacted(r"C:\Users\Ada\x", None), r"C:\Users\Ada\x");
        assert_eq!(redacted(r"C:\Users\Ada\x", Some("")), r"C:\Users\Ada\x");
    }
}
