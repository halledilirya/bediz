use bediz_core::delivery::Delivery;
use bediz_core::geometry::Point;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, HMENU, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    SetMenuDefaultItem, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx,
};
use windows::core::{PCWSTR, Result};

use crate::event::MenuTexts;

const COPY_AND_SAVE: usize = 1;
const COPY: usize = 2;
const SAVE: usize = 3;
const COPY_COLOUR: usize = 4;
const WHOLE_MONITOR: usize = 5;
const LAST_REGION: usize = 6;
const CANCEL: usize = 7;
const SAVE_AS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Deliver(Delivery),
    SaveAs,
    CopyColour,
    WholeMonitor,
    LastRegion,
    Cancel,
}

pub struct Offer {
    pub selected: bool,
    pub last_region: bool,
}

struct OwnedMenu(HMENU);

impl Drop for OwnedMenu {
    fn drop(&mut self) {
        let _ = unsafe { DestroyMenu(self.0) };
    }
}

impl OwnedMenu {
    fn item(&self, id: usize, text: &str, key: &str, enabled: bool) -> Result<()> {
        let label: Vec<u16> = format!("{text}\t{key}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let flags = if enabled {
            MF_STRING
        } else {
            MF_STRING | MF_GRAYED
        };
        unsafe { AppendMenuW(self.0, flags, id, PCWSTR(label.as_ptr())) }
    }

    fn separator(&self) -> Result<()> {
        unsafe { AppendMenuW(self.0, MF_SEPARATOR, 0, PCWSTR::null()) }
    }
}

pub fn choose(owner: HWND, at: Point, texts: &MenuTexts, offer: &Offer) -> Result<Option<Choice>> {
    let menu = OwnedMenu(unsafe { CreatePopupMenu() }?);
    if offer.selected {
        menu.item(COPY_AND_SAVE, &texts.copy_and_save, "Enter", true)?;
        menu.item(COPY, &texts.copy, "Ctrl+C", true)?;
        menu.item(SAVE, &texts.save, "Ctrl+S", true)?;
        menu.item(SAVE_AS, &texts.save_as, "Ctrl+Shift+S", true)?;
        menu.separator()?;
        unsafe { SetMenuDefaultItem(menu.0, COPY_AND_SAVE as u32, 0) }?;
    }
    menu.item(COPY_COLOUR, &texts.copy_colour, "C", true)?;
    menu.item(WHOLE_MONITOR, &texts.whole_monitor, "Ctrl+A", true)?;
    menu.item(LAST_REGION, &texts.last_region, "R", offer.last_region)?;
    menu.separator()?;
    menu.item(CANCEL, &texts.cancel, "Esc", true)?;
    let flags = TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY;
    let chosen = unsafe { TrackPopupMenuEx(menu.0, flags.0, at.x, at.y, owner, None) };
    Ok(choice(chosen.0))
}

fn choice(id: i32) -> Option<Choice> {
    match usize::try_from(id).ok()? {
        COPY_AND_SAVE => Some(Choice::Deliver(Delivery::COPY_AND_SAVE)),
        COPY => Some(Choice::Deliver(Delivery::COPY)),
        SAVE => Some(Choice::Deliver(Delivery::SAVE)),
        SAVE_AS => Some(Choice::SaveAs),
        COPY_COLOUR => Some(Choice::CopyColour),
        WHOLE_MONITOR => Some(Choice::WholeMonitor),
        LAST_REGION => Some(Choice::LastRegion),
        CANCEL => Some(Choice::Cancel),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_menu_chooses_nothing() {
        assert_eq!(choice(0), None);
        assert_eq!(choice(-1), None);
    }

    #[test]
    fn every_item_maps_to_its_choice() {
        assert_eq!(choice(COPY as i32), Some(Choice::Deliver(Delivery::COPY)));
        assert_eq!(choice(LAST_REGION as i32), Some(Choice::LastRegion));
        assert_eq!(choice(CANCEL as i32), Some(Choice::Cancel));
    }
}
