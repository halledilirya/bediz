//! The overlay's menus: the context menu that copies, saves or records the selection, copies its
//! text and sets how the lines are copied, copies a colour, picks the whole monitor or the last
//! region, or cancels, and the colour of marks.

use bediz_core::delivery::Delivery;
use bediz_core::geometry::Point;
use bediz_core::markup::Swatch;
use bediz_core::ocr::Layout;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, HMENU, MF_CHECKED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, SetMenuDefaultItem, TPM_LAYOUTRTL, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTALIGN,
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
const RECORD: usize = 9;
const READ_TEXT: usize = 10;
const COPY_WORDS: usize = 11;
const JOIN_LINES: usize = 12;
const KEEP_INDENTATION: usize = 13;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Deliver(Delivery),
    SaveAs,
    ReadText,
    CopyWords,
    Layout(Layout),
    Record,
    CopyColour,
    WholeMonitor,
    LastRegion,
    Cancel,
}

pub struct Offer {
    pub selected: bool,
    pub last_region: bool,
    pub enter: Delivery,
    pub recording: bool,
    pub picture: bool,
    pub words: Option<Layout>,
}

struct OwnedMenu(HMENU);

impl Drop for OwnedMenu {
    fn drop(&mut self) {
        let _ = unsafe { DestroyMenu(self.0) };
    }
}

impl OwnedMenu {
    fn item(&self, id: usize, text: &str, key: &str, enabled: bool) -> Result<()> {
        self.add(id, &label(text, key), enabled, false)
    }

    fn add(&self, id: usize, label: &str, enabled: bool, checked: bool) -> Result<()> {
        let label: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
        let mut flags = MF_STRING;
        if !enabled {
            flags |= MF_GRAYED;
        }
        if checked {
            flags |= MF_CHECKED;
        }
        unsafe { AppendMenuW(self.0, flags, id, PCWSTR(label.as_ptr())) }
    }

    fn separator(&self) -> Result<()> {
        unsafe { AppendMenuW(self.0, MF_SEPARATOR, 0, PCWSTR::null()) }
    }
}

pub fn choose(owner: HWND, at: Point, texts: &MenuTexts, offer: &Offer) -> Result<Option<Choice>> {
    let menu = OwnedMenu(unsafe { CreatePopupMenu() }?);
    if offer.recording {
        menu.item(RECORD, &texts.record, "Enter", offer.selected)?;
        if offer.selected {
            unsafe { SetMenuDefaultItem(menu.0, RECORD as u32, 0) }?;
        }
        menu.separator()?;
    } else if offer.selected {
        let key = |delivery: Delivery, own: &'static str| {
            if delivery == offer.enter {
                "Enter"
            } else {
                own
            }
        };
        let copy_key = if offer.words.is_some() { "" } else { "Ctrl+C" };
        menu.item(
            COPY_AND_SAVE,
            &texts.copy_and_save,
            key(Delivery::COPY_AND_SAVE, ""),
            true,
        )?;
        menu.item(COPY, &texts.copy, key(Delivery::COPY, copy_key), true)?;
        menu.item(SAVE, &texts.save, key(Delivery::SAVE, "Ctrl+S"), true)?;
        menu.item(SAVE_AS, &texts.save_as, "Ctrl+Shift+S", true)?;
        match offer.words {
            Some(layout) => {
                menu.separator()?;
                menu.item(COPY_WORDS, &texts.read_text, "Ctrl+C", true)?;
                menu.add(JOIN_LINES, &texts.join_lines, true, layout.join_lines)?;
                menu.add(
                    KEEP_INDENTATION,
                    &texts.keep_indentation,
                    !layout.join_lines,
                    layout.keep_indentation,
                )?;
            }
            None => menu.item(READ_TEXT, &texts.read_text, "O", true)?,
        }
        menu.separator()?;
        if !offer.picture {
            menu.item(RECORD, &texts.record, "", true)?;
            menu.separator()?;
        }
        unsafe { SetMenuDefaultItem(menu.0, item_for(offer.enter) as u32, 0) }?;
    }
    if !offer.recording {
        menu.item(COPY_COLOUR, &texts.copy_colour, "C", true)?;
    }
    if !offer.picture {
        let key = if offer.words.is_some() { "" } else { "Ctrl+A" };
        menu.item(WHOLE_MONITOR, &texts.whole_monitor, key, true)?;
        menu.item(LAST_REGION, &texts.last_region, "R", offer.last_region)?;
    }
    menu.separator()?;
    menu.item(CANCEL, &texts.cancel, "Esc", true)?;
    let flags = flags(texts.right_to_left);
    let chosen = unsafe { TrackPopupMenuEx(menu.0, flags.0, at.x, at.y, owner, None) };
    Ok(choice(chosen.0, offer.words.unwrap_or_default()))
}

pub fn choose_colour(
    owner: HWND,
    at: Point,
    texts: &MenuTexts,
    current: Swatch,
) -> Result<Option<Swatch>> {
    let menu = OwnedMenu(unsafe { CreatePopupMenu() }?);
    for (index, swatch) in Swatch::ALL.into_iter().enumerate() {
        let label: Vec<u16> = texts
            .markup
            .swatch(swatch)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let flags = if swatch == current {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        unsafe { AppendMenuW(menu.0, flags, index + 1, PCWSTR(label.as_ptr())) }?;
    }
    let flags = flags(texts.right_to_left);
    let chosen = unsafe { TrackPopupMenuEx(menu.0, flags.0, at.x, at.y, owner, None) };
    Ok(swatch_for(chosen.0))
}

fn swatch_for(id: i32) -> Option<Swatch> {
    let index = usize::try_from(id).ok()?.checked_sub(1)?;
    Swatch::ALL.get(index).copied()
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

fn choice(id: i32, layout: Layout) -> Option<Choice> {
    match usize::try_from(id).ok()? {
        COPY_AND_SAVE => Some(Choice::Deliver(Delivery::COPY_AND_SAVE)),
        COPY => Some(Choice::Deliver(Delivery::COPY)),
        SAVE => Some(Choice::Deliver(Delivery::SAVE)),
        SAVE_AS => Some(Choice::SaveAs),
        READ_TEXT => Some(Choice::ReadText),
        COPY_WORDS => Some(Choice::CopyWords),
        JOIN_LINES => Some(Choice::Layout(Layout {
            join_lines: !layout.join_lines,
            ..layout
        })),
        KEEP_INDENTATION => Some(Choice::Layout(Layout {
            keep_indentation: !layout.keep_indentation,
            ..layout
        })),
        RECORD => Some(Choice::Record),
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
        assert_eq!(choice(0, Layout::default()), None);
        assert_eq!(choice(-1, Layout::default()), None);
        assert_eq!(swatch_for(0), None);
        assert_eq!(swatch_for(-1), None);
    }

    #[test]
    fn every_colour_item_maps_to_its_swatch() {
        for (index, swatch) in Swatch::ALL.into_iter().enumerate() {
            assert_eq!(swatch_for(index as i32 + 1), Some(swatch));
        }
        assert_eq!(swatch_for(Swatch::ALL.len() as i32 + 1), None);
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
        let chosen = |id: usize| choice(id as i32, Layout::default());
        assert_eq!(chosen(COPY), Some(Choice::Deliver(Delivery::COPY)));
        assert_eq!(chosen(LAST_REGION), Some(Choice::LastRegion));
        assert_eq!(chosen(CANCEL), Some(Choice::Cancel));
        assert_eq!(chosen(RECORD), Some(Choice::Record));
        assert_eq!(chosen(READ_TEXT), Some(Choice::ReadText));
        assert_eq!(chosen(COPY_WORDS), Some(Choice::CopyWords));
    }

    #[test]
    fn a_line_switch_turns_its_own_setting_over_and_keeps_the_other() {
        let joined = Layout {
            join_lines: true,
            keep_indentation: true,
        };
        assert_eq!(
            choice(JOIN_LINES as i32, joined),
            Some(Choice::Layout(Layout {
                join_lines: false,
                keep_indentation: true,
            }))
        );
        assert_eq!(
            choice(KEEP_INDENTATION as i32, Layout::default()),
            Some(Choice::Layout(Layout {
                join_lines: false,
                keep_indentation: true,
            }))
        );
    }
}
