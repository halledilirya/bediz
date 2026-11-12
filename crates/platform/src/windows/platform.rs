use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use thiserror::Error;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::runtime::{self, Commands, WM_COMMANDS, WM_FINISH_SAVES, WM_HOLD, WM_STOP};
use crate::event::{Command, Event};

#[derive(Debug, Error)]
pub enum StartError {
    #[error("the platform thread could not be started: {0}")]
    Thread(#[from] std::io::Error),
    #[error("the platform thread stopped before it was ready: {0}")]
    Setup(String),
}

pub struct Platform {
    window: isize,
    commands: Commands,
    saves_finished: Mutex<Receiver<bool>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Platform {
    pub fn start(sink: impl Fn(Event) + Send + 'static) -> Result<Self, StartError> {
        let commands: Commands = Arc::new(Mutex::new(Vec::new()));
        let (ready_sender, ready) = mpsc::channel();
        let (finished_sender, saves_finished) = mpsc::channel();
        let thread_commands = Arc::clone(&commands);
        let thread = std::thread::Builder::new()
            .name("bediz-platform".to_owned())
            .spawn(move || {
                runtime::run(
                    thread_commands,
                    Box::new(sink),
                    ready_sender,
                    finished_sender,
                );
            })?;
        let reply = ready.recv();
        match reply {
            Ok(Ok(window)) => Ok(Self {
                window,
                commands,
                saves_finished: Mutex::new(saves_finished),
                thread: Mutex::new(Some(thread)),
            }),
            Ok(Err(message)) => {
                let _ = thread.join();
                Err(StartError::Setup(message))
            }
            Err(_) => {
                let _ = thread.join();
                Err(StartError::Setup(
                    "the thread ended without a reply".to_owned(),
                ))
            }
        }
    }

    pub fn send(&self, command: Command) -> bool {
        self.commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(command);
        self.post(WM_COMMANDS)
    }

    pub fn finish_saves(&self) {
        let _ = self.ask(WM_FINISH_SAVES);
    }

    pub fn hold(&self) -> bool {
        self.ask(WM_HOLD)
    }

    pub fn resume(&self) {
        self.send(Command::Resume);
    }

    fn ask(&self, message: u32) -> bool {
        let answers = self
            .saves_finished
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.post(message) && answers.recv().unwrap_or(false)
    }

    pub fn shutdown(&self) {
        let thread = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(thread) = thread
            && self.post(WM_STOP)
        {
            let _ = thread.join();
        }
    }

    fn post(&self, message: u32) -> bool {
        let window = HWND(self.window as *mut c_void);
        unsafe { PostMessageW(Some(window), message, WPARAM(0), LPARAM(0)) }.is_ok()
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        self.shutdown();
    }
}
