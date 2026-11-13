//! Quitting: asks first while a GIF is still unsaved, and quits, waits or stays as answered.

use std::sync::mpsc::Sender;

use bediz_platform::{
    Command, Platform, QuitChoice, QuitDialog, QuitQuestion, QuitStep, ask_to_quit,
};
use tauri::{AppHandle, Manager};

use crate::events::Message;
use crate::i18n::Texts;
use crate::setup;
use crate::tray;

pub enum Leaving {
    Asked,
    Shown(QuitDialog),
}

// A platform that cannot be asked holds no GIF to wait for, so Bediz quits at once.
pub fn ask(app: &AppHandle, leaving: &mut Option<Leaving>) {
    if leaving.is_some() {
        return;
    }
    let asked = app
        .try_state::<Platform>()
        .is_some_and(|platform| platform.send(Command::Quit(QuitStep::Ask)));
    if asked {
        *leaving = Some(Leaving::Asked);
    } else {
        app.exit(0);
    }
}

pub fn unsaved(
    leaving: &mut Option<Leaving>,
    percent: Option<u8>,
    texts: &Texts,
    messages: &Sender<Message>,
) {
    let percent = percent.unwrap_or(0);
    match leaving {
        None => {}
        Some(Leaving::Asked) => {
            let dialog = QuitDialog::default();
            dialog.show_percent(percent);
            show(dialog.clone(), question(texts), messages);
            *leaving = Some(Leaving::Shown(dialog));
        }
        Some(Leaving::Shown(dialog)) => dialog.show_percent(percent),
    }
}

pub fn chosen(
    app: &AppHandle,
    leaving: &mut Option<Leaving>,
    dialog: &QuitDialog,
    choice: QuitChoice,
) -> bool {
    if !answers(leaving.as_ref(), dialog) {
        return false;
    }
    let step = match choice {
        QuitChoice::WhenSaved => {
            tray::show_icon(app, false);
            QuitStep::WhenSaved
        }
        QuitChoice::Discard => QuitStep::Discard,
        QuitChoice::Stay => {
            *leaving = None;
            tray::show_icon(app, true);
            QuitStep::Stay
        }
    };
    setup::send(app, Command::Quit(step));
    true
}

// A question closed or replaced meanwhile still sends its last answer, which then changes nothing.
fn answers(leaving: Option<&Leaving>, dialog: &QuitDialog) -> bool {
    matches!(leaving, Some(Leaving::Shown(shown)) if shown.is(dialog))
}

// An answer crossing a Cancel already chosen finds nothing asked, and Bediz stays.
pub fn ready(app: &AppHandle, leaving: &mut Option<Leaving>) {
    match leaving.take() {
        None => return,
        Some(Leaving::Shown(dialog)) => dialog.close(),
        Some(Leaving::Asked) => {}
    }
    app.exit(0);
}

pub fn cancelled(app: &AppHandle, leaving: &mut Option<Leaving>) {
    if let Some(Leaving::Shown(dialog)) = leaving.take() {
        dialog.close();
    }
    tray::show_icon(app, true);
}

pub fn choice_name(choice: QuitChoice) -> &'static str {
    match choice {
        QuitChoice::WhenSaved => "close when it is saved",
        QuitChoice::Discard => "delete it and close",
        QuitChoice::Stay => "stay",
    }
}

fn question(texts: &Texts) -> QuitQuestion {
    QuitQuestion {
        title: texts.get("question-title"),
        heading: texts.get("quit-unsaved"),
        body: texts.get("quit-unsaved-detail"),
        waiting: texts.get("quit-waiting"),
        when_saved: texts.get("quit-when-saved"),
        discard: texts.get("quit-discard"),
        stay: texts.get("quit-stay"),
        right_to_left: texts.right_to_left(),
    }
}

// A question that cannot be shown leaves nothing to answer, so Bediz waits for the GIF and closes.
fn show(dialog: QuitDialog, question: QuitQuestion, messages: &Sender<Message>) {
    let replies = messages.clone();
    let asked = dialog.clone();
    let spawned = std::thread::Builder::new()
        .name("bediz-quit".to_owned())
        .spawn(move || {
            let choose = |choice| {
                let _ = replies.send(Message::QuitChoice(dialog.clone(), choice));
            };
            if let Err(error) = ask_to_quit(&question, &dialog, &choose) {
                let _ = replies.send(Message::Note(format!(
                    "the question before quitting could not be shown: {error}"
                )));
                choose(QuitChoice::WhenSaved);
            }
        });
    if let Err(error) = spawned {
        let _ = messages.send(Message::Note(format!(
            "the question before quitting could not be asked: {error}"
        )));
        let _ = messages.send(Message::QuitChoice(asked, QuitChoice::WhenSaved));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_question_still_shown_is_answered() {
        let shown = QuitDialog::default();
        let earlier = QuitDialog::default();
        let leaving = Leaving::Shown(shown.clone());
        assert!(answers(Some(&leaving), &shown));
        assert!(!answers(Some(&leaving), &earlier));
    }

    #[test]
    fn a_late_answer_after_the_quit_was_given_up_or_cancelled_is_dropped() {
        let dialog = QuitDialog::default();
        assert!(!answers(None, &dialog));
        assert!(!answers(Some(&Leaving::Asked), &dialog));
    }
}
