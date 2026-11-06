use bediz_core::delivery::Delivery;
use bediz_core::geometry::Point;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, HMENU, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    SetMenuDefaultItem, TPM_LAYOUTRTL, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTALIGN,
    TPM_RIGHTBUTTON, TRACK_POPUP_MENU_FLAGS, TrackPopupMenuEx,
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
    pub enter: Delivery,
}

struct OwnedMenu(HMENU);

impl Drop for OwnedMenu {
    fn drop(&mut self) {
        let _ = unsafe { DestroyMenu(self.0) };
    }
}

impl OwnedMenu {
    fn item(&self, id: usize, text: &str, key: &str, enabled: bool) -> Result<()> {
        let label: Vec<u16> = label(text, key)
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
        let key = |delivery: Delivery, own: &'static str| {
            if delivery == offer.enter {
                "Enter"
            } else {
                own
            }
        };
        menu.item(
            COPY_AND_SAVE,
            &texts.copy_and_save,
            key(Delivery::COPY_AND_SAVE, ""),
            true,
        )?;
        menu.item(COPY, &texts.copy, key(Delivery::COPY, "Ctrl+C"), true)?;
        menu.item(SAVE, &texts.save, key(Delivery::SAVE, "Ctrl+S"), true)?;
        menu.item(SAVE_AS, &texts.save_as, "Ctrl+Shift+S", true)?;
        menu.separator()?;
        unsafe { SetMenuDefaultItem(menu.0, item_for(offer.enter) as u32, 0) }?;
    }
    menu.item(COPY_COLOUR, &texts.copy_colour, "C", true)?;
    menu.item(WHOLE_MONITOR, &texts.whole_monitor, "Ctrl+A", true)?;
    menu.item(LAST_REGION, &texts.last_region, "R", offer.last_region)?;
    menu.separator()?;
    menu.item(CANCEL, &texts.cancel, "Esc", true)?;
    let flags = flags(texts.right_to_left);
    let chosen = unsafe { TrackPopupMenuEx(menu.0, flags.0, at.x, at.y, owner, None) };
    Ok(choice(chosen.0))
}

fn flags(right_to_left: bool) -> TRACK_POPUP_MENU_FLAGS {
    let flags = TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY;
    if right_to_left {
        flags | TPM_LAYOUTRTL | TPM_RIGHTALIGN
    } else {
        flags
    }
}

fn label(text: &str, key: &str) -> String {
    if key.is_empty() {
        text.to_owned()
    } else {
        format!("{text}\t{key}")
    }
}

fn item_for(delivery: Delivery) -> usize {
    match delivery {
        Delivery::COPY => COPY,
        Delivery::SAVE => SAVE,
        _ => COPY_AND_SAVE,
    }
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
    fn the_item_enter_does_is_the_default() {
        assert_eq!(item_for(Delivery::COPY_AND_SAVE), COPY_AND_SAVE);
        assert_eq!(item_for(Delivery::COPY), COPY);
        assert_eq!(item_for(Delivery::SAVE), SAVE);
    }

    #[test]
    fn an_item_without_a_key_shows_no_tab() {
        assert_eq!(label("Copy", "Ctrl+C"), "Copy\tCtrl+C");
        assert_eq!(label("Copy and save", ""), "Copy and save");
    }

    #[test]
    fn a_right_to_left_menu_is_mirrored_and_opens_to_the_left() {
        let mirrored = TPM_LAYOUTRTL | TPM_RIGHTALIGN;
        assert_eq!(flags(false).0 & mirrored.0, 0);
        assert_eq!(flags(true).0 & mirrored.0, mirrored.0);
        assert_eq!(flags(true).0 & !mirrored.0, flags(false).0);
    }

    #[test]
    fn every_item_maps_to_its_choice() {
        assert_eq!(choice(COPY as i32), Some(Choice::Deliver(Delivery::COPY)));
        assert_eq!(choice(LAST_REGION as i32), Some(Choice::LastRegion));
        assert_eq!(choice(CANCEL as i32), Some(Choice::Cancel));
    }
}
