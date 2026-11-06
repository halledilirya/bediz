use std::sync::mpsc::Sender;

use bediz_core::hotkey::Hotkey;
use bediz_core::settings::Settings;
use bediz_platform::{Command, HotkeyState, Platform, Question, ask_question};
use tauri::{AppHandle, Manager};

use crate::events::Message;
use crate::i18n::Texts;
use crate::settings;

pub fn ask_about_hotkey(
    app: &AppHandle,
    texts: &Texts,
    state: HotkeyState,
    hotkey: Hotkey,
    messages: &Sender<Message>,
) {
    let snipping_tool = state == HotkeyState::SnippingTool;
    let key = hotkey.to_string();
    let question = if snipping_tool {
        Question {
            title: texts.get("question-title"),
            heading: texts.get("question-snipping-tool"),
            body: texts.get("question-snipping-tool-detail"),
            accept: texts.get("question-snipping-tool-accept"),
            decline: texts.get("question-not-now"),
            never_again: texts.get("question-never-again"),
            right_to_left: texts.right_to_left(),
        }
    } else {
        Question {
            title: texts.get("question-title"),
            heading: texts.get_with("question-taken", "key", &key),
            body: texts.get_with("question-taken-detail", "key", &key),
            accept: texts.get("question-taken-accept"),
            decline: texts.get("question-not-now"),
            never_again: texts.get("question-never-again"),
            right_to_left: texts.right_to_left(),
        }
    };
    let app = app.clone();
    let replies = messages.clone();
    let spawned = std::thread::Builder::new()
        .name("bediz-question".to_owned())
        .spawn(move || {
            let reply = match ask_question(&question) {
                Ok(reply) => reply,
                Err(error) => {
                    let _ = replies.send(Message::Note(format!(
                        "the question about the shortcut could not be shown: {error}"
                    )));
                    return;
                }
            };
            let _ = replies.send(Message::Info(format!(
                "asked about the shortcut accepted={} never_again={}",
                reply.accepted, reply.never_again
            )));
            if reply.never_again {
                settings::change(&app, |current| Settings {
                    ask_about_hotkey: false,
                    ..current.clone()
                });
            }
            match (reply.accepted, snipping_tool) {
                (true, true) => {
                    if let Some(platform) = app.try_state::<Platform>() {
                        platform.send(Command::TakePrintScreen);
                    }
                }
                (true, false) => {
                    let _ = replies.send(Message::OpenSettings);
                }
                (false, _) => {}
            }
        });
    if let Err(error) = spawned {
        let _ = messages.send(Message::Note(format!(
            "the question about the shortcut could not be asked: {error}"
        )));
    }
}
