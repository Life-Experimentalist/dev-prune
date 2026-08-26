// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Internationalisation: the fixed chrome of dev-prune's own output, in the language the
// user asked for.
//
// Deliberately bounded, and the boundary is the whole design. Translated: section
// headings, the summary lines, and the group titles in the configurator — the words that
// repeat on every run and carry no information a script would parse. Never translated:
// `--json`, exit codes, flag names, config keys, adapter names, and the sentence a
// lockfile refusal prints. Those are a contract or a diagnosis; a bug report quoting a
// translated refusal is a bug report nobody upstream can read, and a `--json` document
// whose strings move with `LANG` is not a document.
//
// English is the source of truth. Every other catalogue is overlaid on top of it, so a
// key a translator has not reached yet prints in English rather than printing its own
// name — a half-finished translation degrades to the original instead of to gibberish.
//
// Adding a language is one JSON file and one line in [`CATALOGUES`]. See
// `docs/TRANSLATIONS.md`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::constants;

/// Every catalogue compiled into the binary.
///
/// The only list of languages there is: the code, the names and the review status all
/// come out of the file's own `_meta` block, so adding a language cannot half-happen by
/// updating one list and not the other.
const CATALOGUES: &[&str] = &[
    include_str!("locales/en.json"),
    include_str!("locales/hi.json"),
    include_str!("locales/te.json"),
    include_str!("locales/ta.json"),
    include_str!("locales/kn.json"),
    include_str!("locales/ml.json"),
    include_str!("locales/bn.json"),
    include_str!("locales/mr.json"),
    include_str!("locales/gu.json"),
    include_str!("locales/pa.json"),
    include_str!("locales/sa.json"),
    include_str!("locales/zh.json"),
];

/// What a catalogue says about itself.
#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    /// The IETF subtag the config key and `DEV_PRUNE_LANG` take.
    pub code: String,
    /// The language's name in English, for the moment the choice is confirmed.
    pub english_name: String,
    /// The language's name in itself, for the person making the choice.
    pub native_name: String,
    /// Whether a native speaker has read this file through.
    ///
    /// Recorded rather than assumed, and said out loud where the language is chosen. A
    /// translation nobody has checked is still worth shipping — it is how the first
    /// speaker of that language finds the mistakes — but saying so is the difference
    /// between an invitation and a claim.
    pub reviewed: bool,
}

/// One language's strings, plus what it says about itself.
#[derive(Debug, Clone, Deserialize)]
struct Catalogue {
    #[serde(rename = "_meta")]
    meta: Meta,
    #[serde(flatten)]
    strings: BTreeMap<String, String>,
}

/// The catalogues that parsed, in the order of [`CATALOGUES`].
///
/// A file that does not parse is dropped rather than panicked on: a malformed
/// translation should cost that translation, not the run. `catalogues_all_parse` is what
/// stops one reaching a release.
fn parsed() -> &'static Vec<Catalogue> {
    static PARSED: OnceLock<Vec<Catalogue>> = OnceLock::new();
    PARSED.get_or_init(|| {
        CATALOGUES
            .iter()
            .filter_map(|raw| serde_json::from_str::<Catalogue>(raw).ok())
            .collect()
    })
}

/// The strings actually in use: English, with the chosen language laid over the top.
static ACTIVE: OnceLock<BTreeMap<String, String>> = OnceLock::new();

/// Build the merged table for one language code.
fn merge(code: &str) -> BTreeMap<String, String> {
    let mut merged = parsed()
        .iter()
        .find(|c| c.meta.code == constants::DEFAULT_LANGUAGE)
        .map(|c| c.strings.clone())
        .unwrap_or_default();
    if code != constants::DEFAULT_LANGUAGE
        && let Some(chosen) = parsed().iter().find(|c| c.meta.code == code)
    {
        // Only keys the translator actually filled in. An empty string is a key they
        // opened and left, and falling back is better than printing nothing at all.
        for (key, value) in &chosen.strings {
            if !value.trim().is_empty() {
                merged.insert(key.clone(), value.clone());
            }
        }
    }
    merged
}

/// Choose the language for this process.
///
/// Resolution order, highest first:
///
/// 1. `DEV_PRUNE_LANG`, which governs one invocation and is what a script or a CI job
///    sets when it wants a known language whatever the machine is configured for.
/// 2. the `language` setting, which is the durable answer for this user.
/// 3. English.
///
/// The operating system's own locale is deliberately *not* consulted. A machine set to
/// Hindi has said nothing about what language it wants its build tools in, and a user
/// who has never asked for a translation should not be given a partial one by a
/// variable they did not set.
///
/// Idempotent, and the first call wins — later calls are ignored, so a command that
/// re-reads the registry cannot change the language halfway through its own output.
pub fn init(configured: Option<&str>) {
    let requested = std::env::var(constants::ENV_LANGUAGE)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| configured.map(str::to_string))
        .unwrap_or_else(|| constants::DEFAULT_LANGUAGE.to_string());

    // An unknown code is English rather than an error. This runs before the command
    // does, and refusing to start because of a typo in a cosmetic setting would be a
    // worse failure than the one it reports.
    let code = if language(&requested).is_some() {
        requested
    } else {
        constants::DEFAULT_LANGUAGE.to_string()
    };

    let _ = ACTIVE.set(merge(&code));
}

/// One translated string.
///
/// Falls back to the key itself, which is why every call site passes a literal: a
/// missing key then prints something a maintainer can grep for rather than an empty
/// line. In practice `catalogues_cover_english` makes that unreachable.
pub fn t(key: &'static str) -> &'static str {
    ACTIVE
        .get_or_init(|| merge(constants::DEFAULT_LANGUAGE))
        .get(key)
        .map(String::as_str)
        .unwrap_or(key)
}

/// A translated string with `{name}` placeholders filled in.
///
/// Runtime substitution rather than `format!` because the template is chosen at runtime
/// and `format!` needs a literal. Placeholders are named, not positional, so a
/// translator may reorder them — which some of these languages require.
pub fn tf(key: &'static str, args: &[(&str, &str)]) -> String {
    let mut out = t(key).to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// What a language code resolves to, or `None` if this binary has no catalogue for it.
pub fn language(code: &str) -> Option<&'static Meta> {
    parsed()
        .iter()
        .find(|c| c.meta.code == code)
        .map(|c| &c.meta)
}

/// Every language as `(code, native name)`, for the configurator's picker.
///
/// Pairs rather than bare codes because `te` is not a word anybody reads: the row has to
/// be legible to the person the translation is *for*, who may well not know the subtag.
/// `&'static` so the picker can hold it in a `Copy` control without knowing where it came
/// from.
pub fn choices() -> &'static [(&'static str, &'static str)] {
    static CHOICES: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    CHOICES.get_or_init(|| {
        parsed()
            .iter()
            .map(|c| (c.meta.code.as_str(), c.meta.native_name.as_str()))
            .collect()
    })
}

/// `en English · hi हिन्दी · te తెలుగు · …`, for the error an unknown code earns.
pub fn catalogue_line() -> String {
    choices()
        .iter()
        .map(|(code, native)| format!("{code} {native}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A catalogue that does not parse is silently dropped at runtime, so this is the
    /// only thing standing between a stray comma and a release that quietly speaks
    /// English to everyone who asked for Telugu.
    #[test]
    fn catalogues_all_parse() {
        assert_eq!(
            parsed().len(),
            CATALOGUES.len(),
            "a locale file failed to parse; every entry in CATALOGUES must be valid JSON \
             with a _meta block"
        );
    }

    /// English defines the key set. A translation with a key English does not have is a
    /// string nothing prints — usually a typo in the key, which is invisible at runtime
    /// because the merge simply ignores it.
    #[test]
    fn catalogues_cover_english() {
        let english: Vec<&String> = parsed()
            .iter()
            .find(|c| c.meta.code == constants::DEFAULT_LANGUAGE)
            .expect("en.json must exist")
            .strings
            .keys()
            .collect();

        for catalogue in parsed() {
            for key in catalogue.strings.keys() {
                assert!(
                    english.contains(&key),
                    "{}.json has key `{key}`, which en.json does not",
                    catalogue.meta.code
                );
            }
        }
    }

    /// Codes are the value `devp config set language` takes, so two catalogues claiming
    /// one code would make the setting ambiguous.
    #[test]
    fn codes_are_unique_and_start_with_english() {
        let mut seen = std::collections::BTreeSet::new();
        for (code, _) in choices() {
            assert!(seen.insert(*code), "duplicate language code `{code}`");
        }
        assert_eq!(
            choices().first().map(|(code, _)| *code),
            Some(constants::DEFAULT_LANGUAGE)
        );
    }

    /// Every catalogue names itself in its own language, which is the only string the
    /// picker can show somebody who cannot read the English name.
    #[test]
    fn every_catalogue_names_itself() {
        for (code, _) in choices() {
            let meta = language(code).expect("choices() only lists catalogues that parsed");
            assert!(!meta.english_name.trim().is_empty(), "{code}");
            assert!(!meta.native_name.trim().is_empty(), "{code}");
        }
    }

    #[test]
    fn unknown_language_is_not_supported() {
        assert!(language("en").is_some());
        assert!(language("te").is_some());
        assert!(language("xx").is_none());
        assert!(language("EN").is_none());
    }

    #[test]
    fn placeholders_are_filled_by_name() {
        // The English template is the one under test; a translation may reorder them.
        let filled = tf("run.freed", &[("size", "1.2 GB"), ("count", "7")]);
        assert!(filled.contains("1.2 GB"), "{filled}");
        assert!(filled.contains('7'), "{filled}");
        assert!(!filled.contains('{'), "{filled}");
    }

    /// The reason the catalogues are merged rather than swapped: an unfinished
    /// translation prints English, never the raw key.
    #[test]
    fn an_untranslated_key_falls_back_to_english() {
        let english = merge(constants::DEFAULT_LANGUAGE);
        for (code, _) in choices() {
            for (key, value) in merge(code) {
                assert!(!value.trim().is_empty(), "`{code}` has an empty `{key}`");
                assert!(
                    english.contains_key(&key),
                    "`{code}` invented the key `{key}`"
                );
            }
        }
    }
}
