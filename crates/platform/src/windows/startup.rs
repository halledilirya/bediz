//! Turns starting with Windows on and off, through the package's startup task or the Run key, and
//! tells whether the user turned it off in Windows.

use std::ffi::OsStr;
use std::io;
use std::iter;
use std::os::windows::ffi::OsStrExt;

use windows::ApplicationModel::{StartupTask, StartupTaskState};
use windows::Win32::Foundation::{ERROR_ACCESS_DISABLED_BY_POLICY, ERROR_FILE_NOT_FOUND};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};
use windows::core::{HSTRING, PCWSTR, w};

use super::package;

const RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const NAME: PCWSTR = w!("Bediz");
const APPROVED: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");
// The TaskId of the startup task in app/src-tauri/msix/AppxManifest.xml.
const TASK: &str = "Bediz";

// A packaged app's writes under HKCU stay in a copy only it sees, so Windows would never read its Run value.
pub fn start_with_windows(on: bool) -> io::Result<()> {
    if package::packaged() {
        startup_task(on)
    } else {
        run_value(on)
    }
}

fn startup_task(on: bool) -> io::Result<()> {
    let task = StartupTask::GetAsync(&HSTRING::from(TASK))?.join()?;
    let state = match (on, task.State()?) {
        (true, StartupTaskState::Disabled) => task.RequestEnableAsync()?.join()?,
        (false, StartupTaskState::Enabled) => {
            task.Disable()?;
            StartupTaskState::Disabled
        }
        (_, state) => state,
    };
    if blocked(on, state) {
        return Err(io::Error::from_raw_os_error(
            ERROR_ACCESS_DISABLED_BY_POLICY.0.cast_signed(),
        ));
    }
    Ok(())
}

// Turning it off in Task Manager is the user's choice, which Bediz leaves alone, so only a policy blocks.
fn blocked(on: bool, state: StartupTaskState) -> bool {
    if on {
        state == StartupTaskState::DisabledByPolicy
    } else {
        state == StartupTaskState::EnabledByPolicy
    }
}

pub fn start_turned_off_by_user() -> io::Result<bool> {
    if package::packaged() {
        let task = StartupTask::GetAsync(&HSTRING::from(TASK))?.join()?;
        return Ok(task.State()? == StartupTaskState::DisabledByUser);
    }
    if read()?.is_none() {
        return Ok(false);
    }
    Ok(approval()?.is_some_and(|approval| turned_off(&approval)))
}

// Task Manager's switch for a Run value is undocumented: no value or an even first byte means on.
fn turned_off(approval: &[u8]) -> bool {
    approval.first().is_some_and(|flags| flags % 2 == 1)
}

fn approval() -> io::Result<Option<Vec<u8>>> {
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            APPROVED,
            NAME,
            RRF_RT_REG_BINARY,
            None,
            None,
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok()?;
    let mut data = vec![0u8; size as usize];
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            APPROVED,
            NAME,
            RRF_RT_REG_BINARY,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok()?;
    data.truncate(size as usize);
    Ok(Some(data))
}

fn run_value(on: bool) -> io::Result<()> {
    let current = read()?;
    if !on {
        if current.is_none() {
            return Ok(());
        }
        let deleted = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, NAME) };
        if deleted != ERROR_FILE_NOT_FOUND {
            deleted.ok()?;
        }
        return Ok(());
    }
    let wanted = quoted(std::env::current_exe()?.as_os_str());
    if current.as_deref() == Some(wanted.as_slice()) {
        return Ok(());
    }
    let data: Vec<u16> = wanted.iter().copied().chain(iter::once(0)).collect();
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN,
            NAME,
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            u32::try_from(data.len() * 2).unwrap_or(u32::MAX),
        )
    }
    .ok()?;
    Ok(())
}

fn read() -> io::Result<Option<Vec<u16>>> {
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN,
            NAME,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok()?;
    let mut value = vec![0u16; (size as usize).div_ceil(2)];
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN,
            NAME,
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok()?;
    value.truncate((size as usize / 2).saturating_sub(1));
    Ok(Some(value))
}

fn quoted(path: &OsStr) -> Vec<u16> {
    let quote = u16::from(b'"');
    iter::once(quote)
        .chain(path.encode_wide())
        .chain(iter::once(quote))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_quoted_so_spaces_in_the_path_work() {
        let expected: Vec<u16> = r#""C:\Program Files\Bediz\bediz.exe""#.encode_utf16().collect();
        assert_eq!(
            quoted(OsStr::new(r"C:\Program Files\Bediz\bediz.exe")),
            expected
        );
    }

    #[test]
    fn task_manager_turns_a_run_value_off_with_an_odd_first_byte() {
        let disabled_at = [0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0xD9, 0x01];
        let approval = |flags: u8| [[flags, 0, 0, 0].as_slice(), &disabled_at].concat();
        assert!(turned_off(&approval(3)));
        assert!(turned_off(&approval(7)));
        assert!(!turned_off(&approval(2)));
        assert!(!turned_off(&approval(6)));
        assert!(!turned_off(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert!(!turned_off(&[]));
    }

    #[test]
    fn a_task_turned_off_in_task_manager_stays_off_without_a_failure() {
        assert!(!blocked(true, StartupTaskState::DisabledByUser));
    }

    #[test]
    fn only_a_policy_keeps_the_task_from_following_the_setting() {
        assert!(blocked(true, StartupTaskState::DisabledByPolicy));
        assert!(blocked(false, StartupTaskState::EnabledByPolicy));
        assert!(!blocked(true, StartupTaskState::Enabled));
        assert!(!blocked(true, StartupTaskState::EnabledByPolicy));
        assert!(!blocked(false, StartupTaskState::Disabled));
        assert!(!blocked(false, StartupTaskState::DisabledByUser));
        assert!(!blocked(false, StartupTaskState::DisabledByPolicy));
    }
}
