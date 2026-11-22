//! A pin's own menu: copying and saving it, its zoom and opacity, closing it, and hiding or closing
//! every pin.

use bediz_core::geometry::Point;
use bediz_core::pin::OPACITIES;
use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
use windows::core::Result;

use super::Pin;
use crate::event::MenuTexts;
use crate::windows::overlay_menu::OwnedMenu;

const COPY: usize = 1;
const SAVE: usize = 2;
const SAVE_AS: usize = 3;
const ZOOM_IN: usize = 4;
const ZOOM_OUT: usize = 5;
const ACTUAL_SIZE: usize = 6;
const CLOSE: usize = 7;
const HIDE_ALL: usize = 8;
const CLOSE_ALL: usize = 9;
const FILL: usize = 10;
const OPACITY: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Copy,
    Save,
    SaveAs,
    ZoomIn,
    ZoomOut,
    ActualSize,
    Fill,
    Opacity(u16),
    Close,
    HideAll,
    CloseAll,
}

pub fn choose(pin: &Pin, at: Point, texts: &MenuTexts) -> Result<Option<Choice>> {
    let (pins, look, owner) = (&texts.pins, pin.look(), pin.window());
    let menu = OwnedMenu::popup()?;
    menu.item(COPY, &texts.copy, "Ctrl+C", true)?;
    menu.item(SAVE, &texts.save, "Ctrl+S", true)?;
    menu.item(SAVE_AS, &texts.save_as, "Ctrl+Shift+S", true)?;
    menu.separator()?;
    let zoom = OwnedMenu::popup()?;
    zoom.item(ZOOM_IN, &pins.zoom_in, "+", pin.can_zoom(1))?;
    zoom.item(ZOOM_OUT, &pins.zoom_out, "-", pin.can_zoom(-1))?;
    zoom.item(
        ACTUAL_SIZE,
        &pins.actual_size,
        "Ctrl+0",
        !pin.is_actual_size(),
    )?;
    zoom.item(FILL, &pins.fit_monitor, "", !pin.fills_monitor())?;
    zoom.put_in(&menu, &pins.zoom)?;
    let opacity = OwnedMenu::popup()?;
    for percent in OPACITIES {
        opacity.add(
            OPACITY + usize::from(percent),
            &pins.percent_of(percent),
            true,
            look.opacity == percent,
        )?;
    }
    opacity.put_in(&menu, &pins.opacity)?;
    menu.separator()?;
    menu.item(CLOSE, &pins.close, "Esc", true)?;
    menu.item(HIDE_ALL, &pins.hide_all, "", true)?;
    menu.item(CLOSE_ALL, &pins.close_all, "", true)?;
    // A menu whose owner is not the foreground window stays open after a click elsewhere.
    let _ = unsafe { SetForegroundWindow(owner) };
    Ok(choice(menu.track(owner, at, texts.right_to_left)))
}

fn choice(id: i32) -> Option<Choice> {
    let id = usize::try_from(id).ok()?;
    let chosen = match id {
        COPY => Choice::Copy,
        SAVE => Choice::Save,
        SAVE_AS => Choice::SaveAs,
        ZOOM_IN => Choice::ZoomIn,
        ZOOM_OUT => Choice::ZoomOut,
        ACTUAL_SIZE => Choice::ActualSize,
        FILL => Choice::Fill,
        CLOSE => Choice::Close,
        HIDE_ALL => Choice::HideAll,
        CLOSE_ALL => Choice::CloseAll,
        _ => {
            let percent = u16::try_from(id.checked_sub(OPACITY)?).ok()?;
            return OPACITIES
                .contains(&percent)
                .then_some(Choice::Opacity(percent));
        }
    };
    Some(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_menu_chooses_nothing_and_every_item_its_own_choice() {
        assert_eq!(choice(0), None);
        assert_eq!(choice(-1), None);
        assert_eq!(choice(COPY as i32), Some(Choice::Copy));
        assert_eq!(choice(SAVE_AS as i32), Some(Choice::SaveAs));
        assert_eq!(choice(ACTUAL_SIZE as i32), Some(Choice::ActualSize));
        assert_eq!(choice(FILL as i32), Some(Choice::Fill));
        assert_eq!(choice(CLOSE_ALL as i32), Some(Choice::CloseAll));
    }

    #[test]
    fn every_opacity_offered_is_chosen_as_itself_and_nothing_between() {
        for percent in OPACITIES {
            assert_eq!(
                choice((OPACITY + usize::from(percent)) as i32),
                Some(Choice::Opacity(percent))
            );
        }
        assert_eq!(choice((OPACITY + 55) as i32), None);
        assert_eq!(choice((OPACITY + 5) as i32), None);
    }
}
