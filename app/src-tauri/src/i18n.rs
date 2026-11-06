use bediz_core::notice::Notice;
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use unic_langid::LanguageIdentifier;

const ENGLISH: (&str, &str) = ("en", include_str!("../../../locales/en/bediz.ftl"));
const LOCALES: [(&str, &str); 2] = [
    ENGLISH,
    ("tr", include_str!("../../../locales/tr/bediz.ftl")),
];

pub struct Language(pub &'static str);

pub struct Texts {
    language: &'static str,
    bundle: FluentBundle<FluentResource>,
}

impl Texts {
    pub fn for_languages(preferred: &[String]) -> Self {
        let (code, source) = preferred
            .iter()
            .find_map(|tag| {
                let primary = tag.split('-').next()?;
                LOCALES
                    .iter()
                    .find(|(code, _)| code.eq_ignore_ascii_case(primary))
            })
            .copied()
            .unwrap_or(ENGLISH);
        let language: LanguageIdentifier = code.parse().unwrap_or_default();
        let mut bundle = FluentBundle::new(vec![language]);
        let resource =
            FluentResource::try_new(source.to_owned()).unwrap_or_else(|(resource, _)| resource);
        let _ = bundle.add_resource(resource);
        Self {
            language: code,
            bundle,
        }
    }

    pub const fn language(&self) -> &'static str {
        self.language
    }

    pub fn get(&self, id: &str) -> String {
        self.format(id, None)
    }

    pub fn notice(&self, notice: &Notice) -> String {
        let mut args = FluentArgs::new();
        if let Some((name, value)) = notice.argument() {
            args.set(name, value.to_owned());
        }
        self.format(notice.locale_id(), Some(&args))
    }

    fn format(&self, id: &str, args: Option<&FluentArgs>) -> String {
        let Some(pattern) = self
            .bundle
            .get_message(id)
            .and_then(|message| message.value())
        else {
            return id.to_owned();
        };
        let mut errors = Vec::new();
        self.bundle
            .format_pattern(pattern, args, &mut errors)
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use bediz_core::delivery::{CopyError, DeliveryError, SaveError};

    use super::*;

    const SETTINGS_PAGE: &str = include_str!("../../index.html");
    const SETTINGS_SCRIPTS: [&str; 2] = [
        include_str!("../../src/settings.ts"),
        include_str!("../../src/search.ts"),
    ];

    const USED: [&str; 20] = [
        "settings-title",
        "tray-settings",
        "tray-tooltip",
        "tray-tooltip-hotkey-taken",
        "tray-tooltip-snipping-tool",
        "tray-take-screenshot",
        "tray-hotkey-taken",
        "tray-hotkey-snipping-tool",
        "tray-retry",
        "tray-take-print-screen",
        "tray-open-logs",
        "tray-quit",
        "overlay-menu-copy-and-save",
        "overlay-menu-copy",
        "overlay-menu-save",
        "overlay-menu-save-as",
        "overlay-menu-copy-colour",
        "overlay-menu-whole-monitor",
        "overlay-menu-last-region",
        "overlay-menu-cancel",
    ];

    fn messages(source: &str) -> BTreeMap<String, BTreeSet<String>> {
        let mut messages: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut message: Option<String> = None;
        let mut key: Option<String> = None;
        for line in source.lines() {
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                continue;
            }
            let value = if trimmed.len() == line.len() {
                let (id, value) = line.split_once('=').unwrap_or((line, ""));
                let id = id.trim();
                message = (!id.starts_with(['#', '-'])).then(|| id.to_owned());
                key = message.clone();
                value
            } else if let (Some(id), Some(attribute)) = (&message, trimmed.strip_prefix('.')) {
                let (name, value) = attribute.split_once('=').unwrap_or((attribute, ""));
                key = Some(format!("{id}.{}", name.trim()));
                value
            } else {
                trimmed
            };
            if let Some(key) = &key {
                messages
                    .entry(key.clone())
                    .or_default()
                    .extend(variables(value));
            }
        }
        messages
    }

    fn variables(text: &str) -> impl Iterator<Item = String> + '_ {
        text.split('$').skip(1).filter_map(|rest| {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            name.starts_with(|c: char| c.is_ascii_alphabetic())
                .then_some(name)
        })
    }

    fn format_errors(texts: &Texts, key: &str, variables: &BTreeSet<String>) -> Vec<String> {
        let (id, attribute) = match key.split_once('.') {
            Some((id, attribute)) => (id, Some(attribute)),
            None => (key, None),
        };
        let Some(message) = texts.bundle.get_message(id) else {
            return vec![format!("{key} is missing")];
        };
        let pattern = match attribute {
            Some(name) => message.get_attribute(name).map(|found| found.value()),
            None => message.value(),
        };
        let Some(pattern) = pattern else {
            return vec![format!("{key} has no value")];
        };
        let mut args = FluentArgs::new();
        for variable in variables {
            args.set(variable.clone(), 1);
        }
        let mut errors = Vec::new();
        texts
            .bundle
            .format_pattern(pattern, Some(&args), &mut errors);
        errors
            .iter()
            .map(|error| format!("{key}: {error}"))
            .collect()
    }

    #[test]
    fn the_scanner_finds_messages_attributes_and_variables() {
        let source = "\
# A comment = not a message
-brand = Bediz
copied = Copied { $width } × { $height }
saved =
    Saved to { $folder }

    as { $name }
count = { $count ->
        [one] One file
       *[other] { $count } files
    }
button = Save
    .title = Save to { $folder }
price = Costs $5
";
        let found = messages(source);
        let expected: BTreeMap<String, BTreeSet<String>> = [
            ("copied", vec!["width", "height"]),
            ("saved", vec!["folder", "name"]),
            ("count", vec!["count"]),
            ("button", vec![]),
            ("button.title", vec!["folder"]),
            ("price", vec![]),
        ]
        .into_iter()
        .map(|(key, names)| {
            (
                key.to_owned(),
                names.into_iter().map(str::to_owned).collect(),
            )
        })
        .collect();
        assert_eq!(found, expected);
    }

    #[test]
    fn every_locale_parses() {
        for (code, source) in LOCALES {
            let resource = FluentResource::try_new(source.to_owned());
            assert!(resource.is_ok(), "{code} has syntax errors");
        }
    }

    #[test]
    fn every_locale_has_the_english_messages_with_the_same_variables() {
        let english = messages(ENGLISH.1);
        for (code, source) in LOCALES {
            assert_eq!(messages(source), english, "{code} differs from en");
        }
    }

    #[test]
    fn every_message_formats_without_errors() {
        let english = messages(ENGLISH.1);
        for (code, _) in LOCALES {
            let texts = Texts::for_languages(&[code.to_owned()]);
            let errors: Vec<String> = english
                .iter()
                .flat_map(|(key, variables)| format_errors(&texts, key, variables))
                .collect();
            assert!(errors.is_empty(), "{code}: {errors:?}");
        }
    }

    #[test]
    fn the_app_uses_only_english_messages() {
        let english = messages(ENGLISH.1);
        for id in USED {
            assert!(english.contains_key(id), "en is missing {id}");
        }
    }

    fn quoted_after<'a>(source: &'a str, marker: &'a str) -> impl Iterator<Item = &'a str> {
        source
            .split(marker)
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
    }

    #[test]
    fn the_settings_page_uses_only_english_messages() {
        let english = messages(ENGLISH.1);
        let page: Vec<&str> = quoted_after(SETTINGS_PAGE, "data-l10n-id=\"").collect();
        let scripts: Vec<&str> = SETTINGS_SCRIPTS
            .iter()
            .flat_map(|script| quoted_after(script, "text(\""))
            .collect();
        assert!(page.len() > 5 && scripts.len() >= 3, "{page:?} {scripts:?}");
        for id in page.into_iter().chain(scripts) {
            assert!(english.contains_key(id), "en is missing {id}");
        }
    }

    #[test]
    fn every_searchable_setting_has_english_keywords() {
        let english = messages(ENGLISH.1);
        let searchable: Vec<&str> = quoted_after(SETTINGS_PAGE, "data-search=\"").collect();
        assert!(!searchable.is_empty());
        for id in searchable {
            assert!(english.contains_key(id), "en is missing {id}");
            let keywords = format!("{id}.keywords");
            assert!(english.contains_key(&keywords), "en is missing {keywords}");
        }
    }

    fn every_notice() -> Vec<Notice> {
        let reason = || "reason".to_owned();
        let copy_errors = [CopyError::Busy, CopyError::Other(reason())];
        let save_errors = [
            SaveError::DiskFull,
            SaveError::AccessDenied,
            SaveError::FolderMissing,
            SaveError::InUse,
            SaveError::NoFreeName,
            SaveError::Busy,
            SaveError::Appeared,
            SaveError::Other(reason()),
        ];
        let delivery_errors = [DeliveryError::Busy, DeliveryError::Other(reason())];
        [
            Notice::Copied,
            Notice::Saved,
            Notice::CopiedAndSaved,
            Notice::ColourCopied("#FFA010".to_owned()),
            Notice::CaptureFailed(reason()),
        ]
        .into_iter()
        .chain(copy_errors.map(Notice::CopyFailed))
        .chain(save_errors.map(Notice::SaveFailed))
        .chain(delivery_errors.map(Notice::DeliveryFailed))
        .collect()
    }

    #[test]
    fn every_notice_has_an_english_message_with_its_variable() {
        let english = messages(ENGLISH.1);
        for notice in every_notice() {
            let id = notice.locale_id();
            let Some(variables) = english.get(id) else {
                panic!("en is missing {id}");
            };
            let expected: BTreeSet<String> = notice
                .argument()
                .map(|(name, _)| name.to_owned())
                .into_iter()
                .collect();
            assert_eq!(variables, &expected, "{id} has other variables");
        }
    }

    #[test]
    fn notices_are_formatted_with_their_values() {
        let texts = Texts::for_languages(&["tr-TR".to_owned()]);
        let colour = texts.notice(&Notice::ColourCopied("#FFA010".to_owned()));
        assert!(colour.contains("#FFA010"), "{colour}");
        let reason = texts.notice(&Notice::SaveFailed(SaveError::Other(
            "network gone".to_owned(),
        )));
        assert!(reason.contains("network gone"), "{reason}");
        for notice in every_notice() {
            assert_ne!(texts.notice(&notice), notice.locale_id());
        }
    }

    #[test]
    fn picks_the_first_supported_language() {
        let texts = Texts::for_languages(&["de-DE".to_owned(), "tr-TR".to_owned()]);
        assert_eq!(texts.get("tray-open-logs"), "Günlük klasörünü aç");
    }

    #[test]
    fn falls_back_to_english() {
        let texts = Texts::for_languages(&["de-DE".to_owned()]);
        assert_eq!(texts.get("tray-open-logs"), "Open log folder");
        let texts = Texts::for_languages(&[]);
        assert_eq!(texts.get("tray-quit"), "Quit Bediz");
    }
}
