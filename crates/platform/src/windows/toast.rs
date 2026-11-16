//! Shows a confirmation as a Windows notification while a screen reader runs, since screen readers
//! read those and may miss the label, which never comes to the front.

use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::{DateTime, IReference, PropertyValue};
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETSCREENREADER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{BOOL, HSTRING, Interface, Result, w};

use super::package;

// The installer gives Bediz's Start menu shortcut this ID, and Windows shows a toast only for it.
const APP_ID: &str = "io.github.halledilirya.bediz";
const TAG: &str = "confirmation";
const KEPT_SECONDS: i64 = 20;
const TICKS_PER_SECOND: i64 = 10_000_000;

pub fn screen_reader_running() -> bool {
    reader_flag_set() || narrator_running()
}

fn reader_flag_set() -> bool {
    let mut running = BOOL(0);
    unsafe {
        SystemParametersInfoW(
            SPI_GETSCREENREADER,
            0,
            Some((&mut running as *mut BOOL).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok_and(|()| running.as_bool())
}

// Narrator does not set the screen reader flag; it is known only by the mutex it holds while it runs.
fn narrator_running() -> bool {
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, w!("NarratorRunning")) } {
        Ok(mutex) => {
            let _ = unsafe { CloseHandle(mutex) };
            true
        }
        Err(error) => error.code() == ERROR_ACCESS_DENIED.to_hresult(),
    }
}

pub fn show(lines: &[String]) -> Result<()> {
    let document = XmlDocument::new()?;
    document.LoadXml(&HSTRING::from(content(lines)))?;
    let toast = ToastNotification::CreateToastNotification(&document)?;
    toast.SetTag(&HSTRING::from(TAG))?;
    toast.SetExpirationTime(&expiring()?)?;
    let notifier = if package::packaged() {
        ToastNotificationManager::CreateToastNotifier()?
    } else {
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?
    };
    notifier.Show(&toast)
}

fn expiring() -> Result<IReference<DateTime>> {
    let now = unsafe { GetSystemTimeAsFileTime() };
    let at = DateTime {
        UniversalTime: ticks_after(now.dwHighDateTime, now.dwLowDateTime, KEPT_SECONDS),
    };
    PropertyValue::CreateDateTime(at)?.cast()
}

fn ticks_after(high: u32, low: u32, seconds: i64) -> i64 {
    let now = (i64::from(high) << 32) | i64::from(low);
    now.saturating_add(seconds.saturating_mul(TICKS_PER_SECOND))
}

// A toast takes at most three texts, so every line after the first shares the second.
fn content(lines: &[String]) -> String {
    let (first, rest) = lines
        .split_first()
        .map_or(("", &[][..]), |(first, rest)| (first.as_str(), rest));
    let mut texts = format!("<text>{}</text>", escaped(first));
    if !rest.is_empty() {
        texts.push_str(&format!("<text>{}</text>", escaped(&rest.join("\n"))));
    }
    format!(
        "<toast><visual><binding template=\"ToastGeneric\">{texts}</binding></visual>\
         <audio silent=\"true\"/></toast>"
    )
}

fn escaped(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_line_is_a_text_with_its_markup_characters_escaped() {
        let lines = [
            "Not saved: <C:\\Pictures> & \"Bediz\"'s folder".to_owned(),
            "Copied".to_owned(),
        ];
        assert_eq!(
            content(&lines),
            "<toast><visual><binding template=\"ToastGeneric\">\
             <text>Not saved: &lt;C:\\Pictures&gt; &amp; &quot;Bediz&quot;&apos;s folder</text>\
             <text>Copied</text></binding></visual><audio silent=\"true\"/></toast>"
        );
    }

    #[test]
    fn many_lines_keep_to_the_texts_a_toast_takes_and_lose_none() {
        for count in [3, 4, 10] {
            let lines: Vec<String> = (1..=count).map(|n| format!("Error {n} <&>")).collect();
            let xml = content(&lines);
            assert_eq!(xml.matches("<text>").count(), 2, "{count} lines");
            for n in 1..=count {
                assert!(
                    xml.contains(&format!("Error {n} &lt;&amp;&gt;")),
                    "{count} lines, line {n}"
                );
            }
        }
        assert_eq!(content(&[]).matches("<text>").count(), 1);
    }

    #[test]
    fn it_leaves_the_notification_centre_twenty_seconds_on() {
        assert_eq!(ticks_after(1, 5, 0), (1 << 32) + 5);
        assert_eq!(
            ticks_after(0x01DC_0000, 0, KEPT_SECONDS) - ticks_after(0x01DC_0000, 0, 0),
            200_000_000
        );
    }

    #[test]
    fn the_id_is_the_one_the_installer_gives_the_shortcut() {
        let config = include_str!("../../../../app/src-tauri/tauri.conf.json");
        assert!(config.contains(&format!("\"identifier\": \"{APP_ID}\"")));
    }
}
