use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows::core::{GUID, PWSTR};

pub fn packaged() -> bool {
    family_name().is_some()
}

// A packaged app's new folders under AppData go to its own folder, the only place other apps see them.
pub fn seen_outside(path: &Path) -> PathBuf {
    let redirected = family_name().and_then(|family| {
        let local = known_folder(&FOLDERID_LocalAppData)?;
        let roaming = known_folder(&FOLDERID_RoamingAppData)?;
        redirected(path, &local, &roaming, &family)
    });
    match redirected {
        Some(redirected) if redirected.exists() => redirected,
        _ => path.to_path_buf(),
    }
}

fn redirected(path: &Path, local: &Path, roaming: &Path, family: &str) -> Option<PathBuf> {
    let cache = local.join("Packages").join(family).join("LocalCache");
    if let Ok(rest) = path.strip_prefix(local) {
        return Some(cache.join("Local").join(rest));
    }
    path.strip_prefix(roaming)
        .ok()
        .map(|rest| cache.join("Roaming").join(rest))
}

fn family_name() -> Option<String> {
    let mut length = 0u32;
    if unsafe { GetCurrentPackageFamilyName(&mut length, None) } != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }
    let mut name = vec![0u16; length as usize];
    let read = unsafe { GetCurrentPackageFamilyName(&mut length, Some(PWSTR(name.as_mut_ptr()))) };
    if read != ERROR_SUCCESS {
        return None;
    }
    name.truncate((length as usize).saturating_sub(1));
    String::from_utf16(&name).ok()
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    let path = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?;
    let folder = OsString::from_wide(unsafe { path.as_wide() });
    unsafe { CoTaskMemFree(Some(path.0.cast())) };
    Some(PathBuf::from(folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(path: &str) -> Option<PathBuf> {
        redirected(
            Path::new(path),
            Path::new(r"C:\Users\Ada\AppData\Local"),
            Path::new(r"C:\Users\Ada\AppData\Roaming"),
            "Bediz_1a2b3c4d5e6f7",
        )
    }

    #[test]
    fn a_folder_under_local_app_data_is_found_in_the_package_cache() {
        assert_eq!(
            seen(r"C:\Users\Ada\AppData\Local\io.github.halledilirya.bediz\logs"),
            Some(PathBuf::from(
                r"C:\Users\Ada\AppData\Local\Packages\Bediz_1a2b3c4d5e6f7\LocalCache\Local\io.github.halledilirya.bediz\logs"
            ))
        );
    }

    #[test]
    fn a_folder_under_roaming_app_data_is_found_in_the_package_cache() {
        assert_eq!(
            seen(r"C:\Users\Ada\AppData\Roaming\io.github.halledilirya.bediz"),
            Some(PathBuf::from(
                r"C:\Users\Ada\AppData\Local\Packages\Bediz_1a2b3c4d5e6f7\LocalCache\Roaming\io.github.halledilirya.bediz"
            ))
        );
    }

    #[test]
    fn a_folder_outside_app_data_is_not_moved() {
        assert_eq!(seen(r"C:\Users\Ada\Pictures\Screenshots"), None);
        assert_eq!(seen(r"C:\Users\Ada\AppData\LocalLow\Bediz"), None);
    }
}
