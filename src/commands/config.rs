// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for the `dev-prune config` command.
//
// Supports `get`, `set`, `show`, `update`, `daemon`, and `hook` sub-actions
// for managing global and per-repo workspace settings.

use anyhow::{Result, bail};
use std::path::Path;

use crate::config::{PerRepoConfig, Registry, Settings};
use crate::i18n;
use crate::output;

/// One tunable in the global config: how to read it, how to write it, and what to say
/// about it.
///
/// A table rather than a `match` arm per operation. `get`, `set`, `show` and the
/// first-run walkthrough all iterate this, so a setting cannot be added to one of them
/// and quietly forgotten in the other three — which is how `min_size_mb` shipped with no
/// line in `config show`.
struct Setting {
    key: &'static str,
    /// Which group of the configurator this setting is asked about under.
    ///
    /// Display order is derived from this rather than from the order of the literal
    /// below, so a new setting is filed by what it does instead of by where there
    /// happened to be room for it.
    category: Category,
    /// The release this key first appeared in.
    ///
    /// Not decoration: the first-run marker records the version it was written at, so
    /// comparing the two is how an upgrade knows which settings the user has never been
    /// shown — without keeping a second list of "new in this version" to forget to
    /// update. See [`settings_added_since_review`].
    since: &'static str,
    /// What kind of value this is, so a picker can offer the right control.
    kind: Kind,
    /// One line, shown by the walkthrough and by `config show --help-text`.
    ///
    /// Written for someone who already knows what a lockfile and a build tree are.
    help: &'static str,
    /// The same setting explained to someone who does not.
    ///
    /// Not a second `help` with shorter words: `help` says what the setting *is*, this
    /// says what happens to you if it is on, in the second person, with no jargon and no
    /// flag names. Both are shown together — nobody should have to be the right kind of
    /// expert to answer a question this tool asked them.
    plain: &'static str,
    get: fn(&Settings) -> String,
    set: fn(&mut Settings, &str) -> Result<()>,
}

/// Which part of the configurator a setting belongs to.
///
/// Thirty keys in one column is a list nobody reads to the end of. The order of
/// [`CATEGORIES`] is the order the groups are drawn in, and it is the order the
/// decisions actually arrive in: first the language the rest of the screen is printed
/// in, then what is in scope, what has to be proved before a delete, the build trees
/// that stay off until they are asked for, the shared caches nothing deletes on its
/// own, and only then the two groups about dev-prune running itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    /// The language dev-prune's own headings and summaries are printed in.
    ///
    /// First because every heading under it is printed in whatever this says, which
    /// makes it the one answer that changes how the rest of the screen reads.
    Presentation,
    /// Which repositories, and which directories inside them, are eligible at all.
    Scope,
    /// What has to hold before anything is deleted, and what verification may do.
    Safety,
    /// The opt-in adapters, whose directories come back by recompiling rather than
    /// by downloading.
    BuildTrees,
    /// Machine-wide download caches: reported on, never deleted unasked.
    Caches,
    /// What happens when nobody typed anything.
    Unattended,
    /// Keeping this copy of dev-prune current.
    Updates,
}

impl Category {
    /// The heading drawn above the group, in `devp config show` and in the
    /// configurator. Written as the question the group answers, not as a noun: a
    /// heading that says "Caches" tells you nothing you could not read off the keys.
    ///
    /// The English wording lives in `src/i18n/locales/en.json` with the rest of the
    /// chrome, so translating a heading never means touching Rust.
    fn title(self) -> &'static str {
        match self {
            Category::Presentation => i18n::t("config.category.presentation"),
            Category::Scope => i18n::t("config.category.scope"),
            Category::Safety => i18n::t("config.category.safety"),
            Category::BuildTrees => i18n::t("config.category.build_trees"),
            Category::Caches => i18n::t("config.category.caches"),
            Category::Unattended => i18n::t("config.category.unattended"),
            Category::Updates => i18n::t("config.category.updates"),
        }
    }
}

/// The groups in the order they are drawn.
const CATEGORIES: &[Category] = &[
    Category::Presentation,
    Category::Scope,
    Category::Safety,
    Category::BuildTrees,
    Category::Caches,
    Category::Unattended,
    Category::Updates,
];

/// Every setting, grouped, in display order.
///
/// The single place that decides what order settings are shown in, so `config show`
/// and the configurator cannot drift into two different orders. Within a group the
/// order of [`SETTINGS`] is kept.
fn settings_by_category() -> Vec<(Category, Vec<&'static Setting>)> {
    CATEGORIES
        .iter()
        .map(|&category| {
            (
                category,
                SETTINGS.iter().filter(|s| s.category == category).collect(),
            )
        })
        .collect()
}

/// How a setting should be *asked* about, as opposed to how it is stored.
///
/// Every value round-trips through `get`/`set` as a string either way — this only
/// decides whether the configurator offers a toggle, a number to type, or the adapter
/// checklist. Validation stays in the setters, which are the one place that owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `true` or `false`.
    Toggle,
    /// A whole number, bounded by whatever its own setter enforces.
    Number,
    /// A comma-separated list of adapter names.
    Adapters,
    /// Cache manager names with a number each, as `npm=10,uv=10`.
    ///
    /// The third column of the same checklist [`Kind::AdapterDays`] is the second of:
    /// which ecosystems run, how long each waits, and how big each one's cache may get
    /// are one table, not three screens.
    CacheCaps,
    /// One of a fixed set of values, cycled in place.
    ///
    /// Which values is supplied when the row is built rather than stored here: the only
    /// thing that knows what the options are is the module that owns them.
    Choice,
    /// Adapter names with a number each, as `cargo=60,npm=30`.
    ///
    /// Edited on the same screen as [`Kind::Adapters`] rather than in a field of its
    /// own: which adapters run and how long each waits are one decision made twice,
    /// and splitting them across two rows is how someone switches an adapter on and
    /// never finds the dial that would have made it safe.
    AdapterDays,
}

/// One first-run suggestion: a setting worth turning on, and the reason.
///
/// A table of its own rather than a field on [`Setting`], because a suggestion is not a
/// property of a setting — it is a claim about what most people should do on the day
/// they install this, and the two lists move for different reasons.
struct Recommendation {
    key: &'static str,
    /// Three or four words naming what accepting it turns on.
    label: &'static str,
    /// Why it is suggested — the part `help` and `plain` both leave out.
    why: &'static str,
    /// The value accepting it sets. A string, not a `bool`, so a suggested *number*
    /// needs no new machinery here or in the view.
    value: &'static str,
    /// The second tier: recommended, with one specific thing to understand first.
    cautious: bool,
    /// Whether the value already on the machine counts as having taken the advice, for
    /// the settings where comparing it to [`Recommendation::value`] asks the wrong
    /// question.
    ///
    /// A toggle has two values and the suggested one is the only one that counts.
    /// `cache_max_gb` holds a map, and somebody who capped npm at 4 GiB has taken this
    /// advice already — the advice is "have a ceiling", not "have this number".
    /// Without this, their own figure would be listed as outstanding on every
    /// `devp config show` forever, and `devp config recommended` would overwrite it.
    taken: Option<fn(&str) -> bool>,
}

/// The safe tier, by the name every command that prints it uses.
///
/// Named once, here, because the configurator, `devp config show` and
/// `devp config recommended` all print these two lists — and a tier that is called
/// something different in each of the three is three lists as far as the reader is
/// concerned.
const SAFE_TIER: &str = "Recommended";

/// The second tier: still recommended, still not risky, but with one specific
/// consequence to understand before accepting it.
const CAUTIOUS_TIER: &str = "Recommended, with one thing to know first";

/// What the first run suggests turning on.
///
/// Every entry is off by default and stays off unless the person accepts it, which is
/// the only reason a screen suggesting them is honest. Nothing already on by default
/// belongs here: a checkbox that is already ticked before you arrive teaches people to
/// tick boxes.
const RECOMMENDED: &[Recommendation] = &[
    Recommendation {
        key: "enable_cargo",
        label: "Rust build folders",
        why: "Rust `target/` directories are usually the largest thing on a developer's disk — \
              tens of gigabytes across a handful of old projects. Nothing is lost: `cargo build` \
              rebuilds it, and a project has to sit untouched for 45 days before this one is even \
              considered.",
        value: "true",
        cautious: false,
        taken: None,
    },
    Recommendation {
        key: "enable_gradle",
        label: "Android / Gradle builds",
        why: "`build/` and `.gradle/` grow with every Android build and are never cleaned up by \
              anything else. They come back on the next build, under the same 45-day wait.",
        value: "true",
        cautious: false,
        taken: None,
    },
    Recommendation {
        key: "enable_maven",
        label: "Maven builds",
        why: "Maven `target/` directories accumulate quietly per module, so a multi-module project \
              has several. `mvn package` brings them back.",
        value: "true",
        cautious: false,
        taken: None,
    },
    Recommendation {
        key: "enable_swift",
        label: "Swift builds",
        why: "`.build/` holds compiled modules for every configuration you have ever built, and \
              `swift build` recreates the one you actually use.",
        value: "true",
        cautious: false,
        taken: None,
    },
    Recommendation {
        key: "enable_dart",
        label: "Dart / Flutter caches",
        why: "`.dart_tool/` carries the pub metadata — back in a second — alongside `build_runner` \
              and `flutter_build` caches that are worth real disk space.",
        value: "true",
        cautious: false,
        taken: None,
    },
    Recommendation {
        key: "enable_mix_build",
        label: "Elixir build trees",
        why: "`_build/` holds compiled beam files for every Mix environment you have built, and \
              `mix compile` recreates the one you are working in.",
        value: "true",
        cautious: false,
        taken: None,
    },
    Recommendation {
        key: "enable_vcpkg",
        label: "C / C++ vcpkg trees",
        why: "`vcpkg_installed/` holds libraries vcpkg compiled from source for one \
              project, and `vcpkg install` builds them again from the manifest beside \
              them.",
        value: "true",
        cautious: false,
        taken: None,
    },
    Recommendation {
        key: "enable_cmake_build",
        label: "C / C++ CMake build trees",
        why: "A configured CMake build tree is object files and linked binaries, and \
              `cmake` writes a `CMakeCache.txt` at the top of it that says which sources \
              build it again — so a `build/` you made by hand is left alone.",
        value: "true",
        cautious: false,
        taken: None,
    },
    Recommendation {
        key: "enable_dotnet_build",
        label: ".NET bin/ and obj/ output",
        why: "NuGet's restore writes `obj/project.assets.json` naming the project it \
              restored, so only output `dotnet build` wrote is claimed — a committed \
              `bin/` holding anything of yours is left alone.",
        value: "true",
        cautious: false,
        taken: None,
    },
    Recommendation {
        key: "cache_max_gb",
        label: "Cache size ceilings",
        why: "Every other suggestion here is about one project's folders. This one is about the \
              download caches all of them share, which only ever grow — npm's on the machine this \
              was written on had passed 10 GiB while every project it served fit in a fraction of \
              that. `default=10` is one ceiling for every manager at once. Crossing it deletes \
              nothing: `devp caches` says which manager is over, and `devp caches clear \
              --over-cap all` is still typed by hand. Name a manager to give it a figure of its \
              own — `devp config set cache_max_gb default=10,npm=4`.",
        value: crate::constants::RECOMMENDED_CACHE_CAP,
        cautious: false,
        // Any ceiling at all is the advice taken. See `Recommendation::taken`.
        taken: Some(|current| parse_cache_caps(current).is_ok_and(|caps| !caps.is_empty())),
    },
    Recommendation {
        key: "allow_manifest_rewrite",
        label: "Let cargo and go tidy up",
        why: "Cautious, not risky. The commands that restore a Rust or Go project can also update \
              `Cargo.lock` or `go.mod` — files Git tracks. Nothing is lost and nothing is deleted, \
              but the next `git status` may show a change you did not make by hand. Turn it on if \
              that is fine; leave it off if a clean working tree matters more than a fully \
              automatic restore.",
        value: "true",
        cautious: true,
        taken: None,
    },
];

/// Every global setting, in the order a person would want to be asked about them.
const SETTINGS: &[Setting] = &[
    Setting {
        key: "language",
        category: Category::Presentation,
        since: "1.10.0",
        kind: Kind::Choice,
        help: "Language for dev-prune's own headings and summary lines. `--json`, exit codes, flag names and config keys stay English in every language.",
        plain: "What language dev-prune talks to you in. Only its own headings change — the words you type and anything a script reads stay in English, so nothing breaks. Everything but English is a community translation, and some have not been proofread yet.",
        get: |s| s.language.clone(),
        set: |s, v| {
            let code = v.trim();
            let Some(meta) = i18n::language(code) else {
                bail!(
                    "unknown language `{code}` — available: {}",
                    i18n::catalogue_line()
                );
            };
            s.language = meta.code.clone();
            Ok(())
        },
    },
    Setting {
        key: "idle_days",
        category: Category::Scope,
        since: "1.0.0",
        kind: Kind::Number,
        help: "Days a repository must sit untouched before it is eligible for pruning.",
        plain: "How long a project has to sit untouched before dev-prune will clean it. Something you worked on yesterday is never touched.",
        get: |s| s.idle_days.to_string(),
        set: |s, v| {
            s.idle_days = v
                .parse()
                .map_err(|_| anyhow::anyhow!("idle_days must be a whole number of days"))?;
            Ok(())
        },
    },
    Setting {
        key: "min_size_mb",
        category: Category::Scope,
        since: "1.0.0",
        kind: Kind::Number,
        help: "Smallest bloat directory worth deleting, in MiB. 0 removes the floor.",
        plain: "Ignore small folders. Deleting a 2 MB folder is not worth the download to get it back.",
        get: |s| s.min_size_mb.to_string(),
        set: |s, v| {
            s.min_size_mb = v.parse().map_err(|_| {
                anyhow::anyhow!("min_size_mb must be a whole number of MiB (0 disables the floor)")
            })?;
            Ok(())
        },
    },
    Setting {
        key: "scan_depth",
        category: Category::Scope,
        since: "1.0.0",
        kind: Kind::Number,
        help: "How many directory levels below a repo root project discovery descends.",
        plain: "How deep inside a repository to look for projects. Raise it if your projects live several folders down; lower it if scanning feels slow.",
        get: |s| s.scan_depth.to_string(),
        set: |s, v| {
            let depth: usize = v
                .parse()
                .map_err(|_| anyhow::anyhow!("scan_depth must be a positive integer"))?;
            // Rejected rather than clamped. `clamp_depth` exists so a hand-edited config
            // file cannot break the walk, but when someone types the number at us we owe
            // them the truth instead of silently storing something else.
            if depth == 0 {
                bail!("scan_depth must be at least 1 — 0 would find no projects at all.");
            }
            if depth > crate::constants::MAX_SCAN_DEPTH_LIMIT {
                bail!(
                    "scan_depth must be at most {} — deeper walks stall on generated trees.",
                    crate::constants::MAX_SCAN_DEPTH_LIMIT
                );
            }
            s.scan_depth = depth;
            Ok(())
        },
    },
    Setting {
        key: "require_confirmation",
        category: Category::Safety,
        since: "1.0.0",
        kind: Kind::Toggle,
        help: "Ask before deleting anything. Turning this off makes every run unattended.",
        plain: "Whether dev-prune asks \"delete these?\" before it deletes. Leave this on unless you want it to run silently while you are away.",
        get: |s| s.require_confirmation.to_string(),
        set: |s, v| {
            s.require_confirmation = parse_bool("require_confirmation", v)?;
            Ok(())
        },
    },
    Setting {
        key: "allow_manifest_rewrite",
        category: Category::Safety,
        since: "1.0.0",
        kind: Kind::Toggle,
        help: "Let cargo and go run the sync command that rewrites tracked manifests.",
        plain: "Lets dev-prune run the command that puts a Rust or Go project back together — which can edit files that are checked into Git. Nothing is lost, but the change shows up in `git status`.",
        get: |s| s.allow_manifest_rewrite.to_string(),
        set: |s, v| {
            s.allow_manifest_rewrite = parse_bool("allow_manifest_rewrite", v)?;
            Ok(())
        },
    },
    Setting {
        key: "command_timeout_secs",
        category: Category::Safety,
        since: "1.0.0",
        kind: Kind::Number,
        help: "How long one package-manager command may run before it is killed — the lockfile check and `devp restore`, never a recompile.",
        plain: "How long to wait for a package manager to answer before giving up on it: the lockfile check before a delete, and the reinstall `devp restore` runs. Nothing is compiled under it — the opt-in build adapters run no command at all during a prune — except a restore whose install builds a native module. Raise it on a slow connection.",
        get: |s| s.command_timeout_secs.to_string(),
        set: |s, v| {
            let secs: u64 = v
                .parse()
                .map_err(|_| anyhow::anyhow!("command_timeout_secs must be a positive integer"))?;
            // Zero is not "no limit": the runner compares elapsed time against it before
            // the child has had a chance to finish, so every lockfile sync would be
            // killed on the spot and nothing would ever be pruneable.
            if secs == 0 {
                bail!(
                    "command_timeout_secs must be at least 1 — 0 would kill every command \
                     the instant it starts."
                );
            }
            s.command_timeout_secs = secs;
            Ok(())
        },
    },
    Setting {
        key: "auto_setup",
        category: Category::Unattended,
        since: "1.0.0",
        kind: Kind::Toggle,
        help: "Install missing integrations by itself, once per installed version.",
        plain: "Whether dev-prune finishes setting itself up on its own instead of making you run `devp setup`.",
        get: |s| s.auto_setup.to_string(),
        set: |s, v| {
            s.auto_setup = parse_bool("auto_setup", v)?;
            Ok(())
        },
    },
    Setting {
        key: "auto_config",
        category: Category::Unattended,
        since: "1.3.0",
        kind: Kind::Toggle,
        help: "Write a default .devprune.json into repositories that link/init register.",
        plain: "Drops a small settings file into each repository you register, so you can give that one project different rules later.",
        get: |s| s.auto_config.to_string(),
        set: |s, v| {
            s.auto_config = parse_bool("auto_config", v)?;
            Ok(())
        },
    },
    Setting {
        key: "auto_daemon",
        category: Category::Unattended,
        since: "1.0.0",
        kind: Kind::Toggle,
        help: "Register the OS scheduler so passes run without being remembered.",
        plain: "Lets your operating system run dev-prune on a schedule, so you never have to remember to.",
        get: |s| s.auto_daemon.to_string(),
        set: |s, v| {
            s.auto_daemon = parse_bool("auto_daemon", v)?;
            Ok(())
        },
    },
    Setting {
        key: "check_interval_days",
        category: Category::Unattended,
        since: "1.0.0",
        kind: Kind::Number,
        help: "Days between scheduled background passes.",
        plain: "How often that scheduled cleanup runs.",
        get: |s| s.check_interval_days.to_string(),
        set: |s, v| {
            let days: u64 = v
                .parse()
                .map_err(|_| anyhow::anyhow!("check_interval_days must be a positive integer"))?;
            // Zero would schedule a prune pass with no gap between passes.
            if days == 0 {
                bail!("check_interval_days must be at least 1.");
            }
            s.check_interval_days = days;
            Ok(())
        },
    },
    Setting {
        key: "auto_discover",
        category: Category::Unattended,
        since: "1.14.0",
        kind: Kind::Toggle,
        help: "Let the scheduled pass register repositories no Git hook could see — unzipped, copied or restored ones.",
        plain: "Finds projects you never added by looking beside the ones you already have, so a repository you unzipped or copied from another machine still gets cleaned up.",
        get: |s| s.auto_discover.to_string(),
        set: |s, v| {
            s.auto_discover = parse_bool("auto_discover", v)?;
            Ok(())
        },
    },
    Setting {
        key: "auto_hooks",
        category: Category::Unattended,
        since: "1.0.0",
        kind: Kind::Toggle,
        help: "Install the Git hooks that register a repository when you clone, commit or merge in it.",
        plain: "Adds repositories as Git creates them. Anything Git did not create — a copied or unzipped project — is left to `auto_discover`.",
        get: |s| s.auto_hooks.to_string(),
        set: |s, v| {
            s.auto_hooks = parse_bool("auto_hooks", v)?;
            Ok(())
        },
    },
    Setting {
        key: "auto_hooks_chain",
        category: Category::Unattended,
        since: "1.0.0",
        kind: Kind::Toggle,
        help: "If another tool owns core.hooksPath, install in front of it and forward. Off by default: that slot is machine-wide and already someone else's.",
        plain: "Git only has one slot for this kind of automation. If something else — husky, pre-commit, lefthook — is already using it, share the slot instead of taking it over. Off by default because the slot is global to your machine and dev-prune would be taking over another tool's setup to use it. `devp doctor` names the command when it finds one of those tools holding it.",
        get: |s| s.auto_hooks_chain.to_string(),
        set: |s, v| {
            s.auto_hooks_chain = parse_bool("auto_hooks_chain", v)?;
            Ok(())
        },
    },
    Setting {
        key: "update_check",
        category: Category::Updates,
        since: "1.0.0",
        kind: Kind::Toggle,
        help: "Ask GitHub for the latest release from time to time. Sends nothing but the request.",
        plain: "Whether dev-prune checks GitHub now and then to see if there is a newer version. It sends no information about you.",
        get: |s| s.update_check.to_string(),
        set: |s, v| {
            s.update_check = parse_bool("update_check", v)?;
            Ok(())
        },
    },
    Setting {
        key: "update_check_interval_days",
        category: Category::Updates,
        since: "1.0.0",
        kind: Kind::Number,
        help: "Days between automatic release checks.",
        plain: "How often that version check happens.",
        get: |s| s.update_check_interval_days.to_string(),
        set: |s, v| {
            let days: i64 = v.parse().map_err(|_| {
                anyhow::anyhow!("update_check_interval_days must be a positive integer")
            })?;
            if days < 1 {
                bail!("update_check_interval_days must be at least 1.");
            }
            s.update_check_interval_days = days;
            Ok(())
        },
    },
    Setting {
        key: "update_check_timeout_secs",
        category: Category::Updates,
        since: "1.0.0",
        kind: Kind::Number,
        help: "Seconds the release check waits for GitHub. Raise it behind a slow proxy.",
        plain: "How long the version check waits before giving up. Raise it if you are behind a slow proxy.",
        get: |s| s.update_check_timeout_secs.to_string(),
        set: |s, v| {
            let secs: u64 = v.parse().map_err(|_| {
                anyhow::anyhow!("update_check_timeout_secs must be a positive integer")
            })?;
            if secs == 0 {
                bail!("update_check_timeout_secs must be at least 1.");
            }
            s.update_check_timeout_secs = secs;
            Ok(())
        },
    },
    Setting {
        key: "enable_cargo",
        category: Category::BuildTrees,
        since: "1.5.0",
        kind: Kind::Toggle,
        help: "Turn on the opt-in Cargo adapter (target/ comes back by recompiling, not downloading).",
        plain: "Clean Rust build folders too. These come back by recompiling, which takes minutes rather than a download — so it is off by default.",
        get: |s| s.enable_cargo.to_string(),
        set: |s, v| {
            s.enable_cargo = parse_bool("enable_cargo", v)?;
            Ok(())
        },
    },
    Setting {
        key: "enable_gradle",
        category: Category::BuildTrees,
        since: "1.3.0",
        kind: Kind::Toggle,
        help: "Turn on the opt-in Gradle adapter (build/ and .gradle/ come back by recompiling).",
        plain: "Clean Android and Java build folders too. Same trade: they come back by recompiling, not downloading.",
        get: |s| s.enable_gradle.to_string(),
        set: |s, v| {
            s.enable_gradle = parse_bool("enable_gradle", v)?;
            Ok(())
        },
    },
    Setting {
        key: "enable_maven",
        category: Category::BuildTrees,
        since: "1.3.0",
        kind: Kind::Toggle,
        help: "Turn on the opt-in Maven adapter (target/ comes back by recompiling).",
        plain: "Clean Maven build folders too. They come back by recompiling.",
        get: |s| s.enable_maven.to_string(),
        set: |s, v| {
            s.enable_maven = parse_bool("enable_maven", v)?;
            Ok(())
        },
    },
    Setting {
        key: "enable_swift",
        category: Category::BuildTrees,
        since: "1.4.0",
        kind: Kind::Toggle,
        help: "Turn on the opt-in SwiftPM adapter (.build/ comes back by recompiling).",
        plain: "Clean Swift build folders too. They come back by recompiling.",
        get: |s| s.enable_swift.to_string(),
        set: |s, v| {
            s.enable_swift = parse_bool("enable_swift", v)?;
            Ok(())
        },
    },
    Setting {
        key: "enable_dart",
        category: Category::BuildTrees,
        since: "1.6.0",
        kind: Kind::Toggle,
        help: "Turn on the opt-in Dart/Flutter adapter (.dart_tool/ holds build caches).",
        plain: "Clean Dart and Flutter caches too. Part comes back instantly, part by recompiling.",
        get: |s| s.enable_dart.to_string(),
        set: |s, v| {
            s.enable_dart = parse_bool("enable_dart", v)?;
            Ok(())
        },
    },
    Setting {
        key: "enable_mix_build",
        category: Category::BuildTrees,
        since: "1.7.0",
        kind: Kind::Toggle,
        help: "Turn on the opt-in Elixir Mix build-tree adapter (_build/ comes back by recompiling).",
        plain: "Elixir projects only. Mix is Elixir's build tool, and it compiles your project and every dependency into `_build/` — this cleans that folder. The downloaded `deps/` folder beside it belongs to a different adapter that is already on. Off by default, because `_build/` comes back by recompiling rather than by downloading.",
        get: |s| s.enable_mix_build.to_string(),
        set: |s, v| {
            s.enable_mix_build = parse_bool("enable_mix_build", v)?;
            Ok(())
        },
    },
    Setting {
        key: "enable_vcpkg",
        category: Category::BuildTrees,
        since: "1.8.0",
        kind: Kind::Toggle,
        help: "Turn on the opt-in vcpkg adapter (vcpkg_installed/ comes back by recompiling).",
        plain: "Clean C and C++ vcpkg_installed/ folders too. They come back by recompiling.",
        get: |s| s.enable_vcpkg.to_string(),
        set: |s, v| {
            s.enable_vcpkg = parse_bool("enable_vcpkg", v)?;
            Ok(())
        },
    },
    Setting {
        key: "enable_cmake_build",
        category: Category::BuildTrees,
        since: "1.8.0",
        kind: Kind::Toggle,
        help: "Turn on the opt-in CMake adapter (build trees proven by their CMakeCache.txt).",
        plain: "Clean C and C++ build folders CMake configured. A `build/` you made by hand is \
                never touched.",
        get: |s| s.enable_cmake_build.to_string(),
        set: |s, v| {
            s.enable_cmake_build = parse_bool("enable_cmake_build", v)?;
            Ok(())
        },
    },
    Setting {
        key: "enable_dotnet_build",
        category: Category::BuildTrees,
        since: "1.18.0",
        kind: Kind::Toggle,
        help: "Turn on the opt-in .NET adapter (bin/ and obj/ proven by NuGet's project.assets.json).",
        plain: "Clean .NET bin/ and obj/ folders MSBuild wrote. A `bin/` holding anything of \
                yours is never touched.",
        get: |s| s.enable_dotnet_build.to_string(),
        set: |s, v| {
            s.enable_dotnet_build = parse_bool("enable_dotnet_build", v)?;
            Ok(())
        },
    },
    Setting {
        key: "build_idle_days",
        category: Category::BuildTrees,
        since: "1.3.0",
        kind: Kind::Number,
        help: "Idle days before the opt-in adapters' build trees are pruned. Applied as max(this, idle_days).",
        plain: "A longer wait, used only for the build folders above, because getting those back costs a recompile rather than a download.",
        get: |s| s.build_idle_days.to_string(),
        set: |s, v| {
            let days: u64 = v
                .parse()
                .map_err(|_| anyhow::anyhow!("build_idle_days must be a non-negative integer"))?;
            s.build_idle_days = days;
            Ok(())
        },
    },
    Setting {
        key: "auto_update",
        category: Category::Updates,
        since: "1.3.0",
        kind: Kind::Toggle,
        help: "Install a newer release by itself at the end of a prune pass. On by default.",
        plain: "Whether dev-prune installs its own updates after a cleanup. The download is checked against its published fingerprint first.",
        get: |s| s.auto_update.to_string(),
        set: |s, v| {
            s.auto_update = parse_bool("auto_update", v)?;
            Ok(())
        },
    },
    Setting {
        key: "version_lock",
        category: Category::Updates,
        since: "1.8.0",
        kind: Kind::Toggle,
        help: "Pin this copy to the version it is. Overrides auto_update, `devp update \
                --install`, `devp install --channel` and the install scripts.",
        plain: "Stay on exactly this version. Nothing dev-prune does replaces the binary \
                while this is on -- not the automatic update, not a re-run of the install \
                one-liner.",
        get: |s| s.version_lock.to_string(),
        set: |s, v| {
            s.version_lock = parse_bool("version_lock", v)?;
            Ok(())
        },
    },
    Setting {
        key: "disabled_adapters",
        category: Category::Scope,
        since: "1.4.0",
        kind: Kind::Adapters,
        help: "Adapters to leave alone entirely, by name. Empty means every one of them is active.",
        plain: "Ecosystems to ignore completely — as if you did not have them installed at all.",
        get: |s| {
            if s.disabled_adapters.is_empty() {
                "(none)".to_string()
            } else {
                s.disabled_adapters.join(",")
            }
        },
        set: |s, v| {
            s.disabled_adapters = parse_adapter_list(v)?;
            Ok(())
        },
    },
    Setting {
        key: "adapter_idle_days",
        category: Category::Scope,
        since: "1.5.0",
        kind: Kind::AdapterDays,
        help: "Per-adapter idle windows, as `cargo=60,npm=30`. Each one can only raise its own wait.",
        plain: "A different waiting period for one ecosystem. Useful when your Rust projects should wait longer than your Node ones.",
        get: |s| {
            if s.adapter_idle_days.is_empty() {
                "(none)".to_string()
            } else {
                s.adapter_idle_days
                    .iter()
                    .map(|(name, days)| format!("{name}={days}"))
                    .collect::<Vec<_>>()
                    .join(",")
            }
        },
        set: |s, v| {
            s.adapter_idle_days = parse_adapter_days(v)?;
            Ok(())
        },
    },
    Setting {
        key: "cache_max_gb",
        category: Category::Caches,
        since: "1.8.0",
        kind: Kind::CacheCaps,
        help: "Per-manager cache size caps in GiB, as `npm=10,uv=10`. Reported by `devp caches`; cleared only by `devp caches clear --over-cap`.",
        plain: "How big one ecosystem's download cache is allowed to get before dev-prune says so. It still never deletes a cache on its own.",
        get: |s| {
            if s.cache_max_gb.is_empty() {
                "(none)".to_string()
            } else {
                s.cache_max_gb
                    .iter()
                    .map(|(name, gb)| format!("{name}={gb}"))
                    .collect::<Vec<_>>()
                    .join(",")
            }
        },
        set: |s, v| {
            s.cache_max_gb = parse_cache_caps(v)?;
            Ok(())
        },
    },
];

/// Parse the comma-separated adapter deny-list, rejecting names that do not exist.
///
/// An unknown name is an error listing the valid ones rather than a no-op, for the same
/// reason `--only nmp` is: a silently ignored typo reads as "npm is protected" right up
/// until the pass that deletes `node_modules`.
/// Parse `npm=10,uv=10` into the per-manager cache cap map.
///
/// [`constants::CACHE_CAP_DEFAULT_KEY`] is accepted alongside the manager names and
/// covers every manager that is not named separately, so a ceiling can be set without
/// first learning which thirty-one caches exist.
///
/// Validated against the cache manager names `devp caches clear` takes, not the adapter
/// names [`parse_adapter_days`] uses. The two lists overlap but neither contains the
/// other — `pip`, `nuget`, `conan`, `conda`, `vcpkg` and `hex` are caches with no
/// adapter, and `venv`, `terraform` and `dart` are adapters with no cache — so
/// accepting an adapter name here would store a cap that nothing ever reads.
///
/// Zero is rejected rather than treated as "cap everything": a cache is over a cap of
/// zero the moment it exists, and a setting whose only effect is to mark every cache
/// permanently over-size is a typo for `-` every time.
fn parse_cache_caps(value: &str) -> Result<std::collections::BTreeMap<String, u64>> {
    let trimmed = value.trim();
    if trimmed.is_empty() || matches!(trimmed.to_lowercase().as_str(), "none" | "(none)" | "-") {
        return Ok(std::collections::BTreeMap::new());
    }

    let mut caps = std::collections::BTreeMap::new();
    for raw in trimmed.split(',') {
        let entry = raw.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((name, value)) = entry.split_once('=') else {
            bail!("`{entry}` must be written as `<manager>=<gib>`, for example `uv=10`.");
        };
        let name = name.trim().to_lowercase();
        if name != crate::constants::CACHE_CAP_DEFAULT_KEY
            && !crate::commands::caches::is_cache_manager(&name)
        {
            bail!(
                "`{name}` is not a manager dev-prune knows a cache for. Valid names: {}. \
                 `{}` caps every manager that is not named separately.",
                crate::commands::caches::known_managers().join(", "),
                crate::constants::CACHE_CAP_DEFAULT_KEY
            );
        }
        let parsed: u64 = value.trim().parse().map_err(|_| {
            anyhow::anyhow!(
                "`{name}` needs a whole number of gibibytes, not `{}`.",
                value.trim()
            )
        })?;
        if parsed == 0 {
            bail!(
                "`{name}=0` would call the cache too big the moment it exists. Use `-` to clear the caps instead."
            );
        }
        caps.insert(name, parsed);
    }
    Ok(caps)
}

/// Parse `cargo=60,npm=30` into the per-adapter idle map.
///
/// Same "clear it" spellings as [`parse_adapter_list`], and the same closed loop: what
/// `config get adapter_idle_days` prints is accepted verbatim by `config set`.
fn parse_adapter_days(value: &str) -> Result<std::collections::BTreeMap<String, u64>> {
    let trimmed = value.trim();
    if trimmed.is_empty() || matches!(trimmed.to_lowercase().as_str(), "none" | "(none)" | "-") {
        return Ok(std::collections::BTreeMap::new());
    }

    let mut days = std::collections::BTreeMap::new();
    for raw in trimmed.split(',') {
        let entry = raw.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((name, value)) = entry.split_once('=') else {
            bail!("`{entry}` must be written as `<adapter>=<days>`, for example `cargo=60`.");
        };
        let name = name.trim().to_lowercase();
        if !crate::adapters::is_adapter_name(&name) {
            bail!(
                "`{name}` is not an adapter. Valid names: {}",
                crate::adapters::all_adapter_names().join(", ")
            );
        }
        let parsed: u64 = value.trim().parse().map_err(|_| {
            anyhow::anyhow!(
                "`{name}` needs a whole number of days, not `{}`.",
                value.trim()
            )
        })?;
        days.insert(name, parsed);
    }
    Ok(days)
}

fn parse_adapter_list(value: &str) -> Result<Vec<String>> {
    let trimmed = value.trim();
    // The spellings that mean "clear it". `(none)` closes the loop with the getter, so
    // whatever `config get` prints can be handed straight back to `config set`.
    if trimmed.is_empty() || matches!(trimmed.to_lowercase().as_str(), "none" | "(none)" | "-") {
        return Ok(Vec::new());
    }

    let mut names: Vec<String> = Vec::new();
    for raw in trimmed.split(',') {
        let name = raw.trim().to_lowercase();
        if name.is_empty() {
            continue;
        }
        if !crate::adapters::is_adapter_name(&name) {
            bail!(
                "`{name}` is not an adapter. Valid names: {}",
                crate::adapters::all_adapter_names().join(", ")
            );
        }
        if !names.contains(&name) {
            names.push(name);
        }
    }
    Ok(names)
}

fn parse_bool(key: &str, value: &str) -> Result<bool> {
    match value.trim().to_lowercase().as_str() {
        "true" | "yes" | "y" | "on" | "1" => Ok(true),
        "false" | "no" | "n" | "off" | "0" => Ok(false),
        _ => bail!("{key} must be true or false"),
    }
}

/// Every stored setting that its own setter would refuse, with the reason.
///
/// `devp config set` guards the ranges, but nothing guards a hand-edited `registry.json`
/// — and the values that get in that way are the quiet ones: `scan_depth: 0` finds no
/// projects, `command_timeout_secs: 0` kills every lockfile command the instant it
/// starts. Both leave a tool that runs, reports success and prunes nothing.
///
/// Round-tripping each value through the setter that owns it is deliberate. A separate
/// list of ranges would be a second copy of the rules, free to drift from the ones
/// actually enforced.
pub fn invalid_settings(settings: &Settings) -> Vec<(&'static str, String)> {
    SETTINGS
        .iter()
        .filter_map(|setting| {
            let mut probe = settings.clone();
            (setting.set)(&mut probe, &(setting.get)(settings))
                .err()
                .map(|e| (setting.key, e.to_string()))
        })
        .collect()
}

/// The number of settings [`invalid_settings`] checks, for reports that say so.
pub fn setting_count() -> usize {
    SETTINGS.len()
}

/// Apply the wizard's settings edits to a registry freshly read from disk, and save
/// that.
///
/// The wizard holds the registry it loaded when it opened, and it can sit open for
/// minutes — long enough for a scheduled pass to finish and record its prune history
/// through its own load–save. Saving the wizard's stale copy wrote that history back
/// out of existence, so only the keys the user actually changed are carried over,
/// onto whatever the file holds now.
fn save_settings_edits<'a>(edits: impl Iterator<Item = (&'a str, &'a str)>) -> Result<()> {
    let mut fresh = Registry::load()?;
    for (key, value) in edits {
        (find_setting(key)?.set)(&mut fresh.settings, value)?;
    }
    fresh.save()
}

fn find_setting(key: &str) -> Result<&'static Setting> {
    SETTINGS
        .iter()
        .find(|s| s.key == key)
        .ok_or_else(|| anyhow::anyhow!("Unknown config key: {key}. Valid keys: {}", valid_keys()))
}

fn valid_keys() -> String {
    SETTINGS
        .iter()
        .map(|s| s.key)
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a `daemon` / `hook` sub-action word means.
#[derive(Debug, PartialEq, Eq)]
pub enum Toggle {
    Enable,
    Disable,
    Status,
}

/// Resolve the sub-action word users actually type.
///
/// `install` / `uninstall` are what this tool's own output and its documentation have
/// always called these operations, and `on` / `off` is the obvious guess; each pair
/// means the same thing as `enable` / `disable`, so all of them are accepted.
///
/// Anything else is an error rather than a fall-through to `status`. Silently printing
/// status for `devp config daemon enabel` looks like it worked and leaves the daemon
/// uninstalled.
pub fn parse_toggle(action: &str) -> Result<Toggle> {
    match action.to_lowercase().as_str() {
        "enable" | "install" | "on" => Ok(Toggle::Enable),
        "disable" | "uninstall" | "remove" | "off" => Ok(Toggle::Disable),
        "" | "status" | "show" => Ok(Toggle::Status),
        other => bail!(
            "Unknown action `{other}`. Expected `enable`, `disable` or `status` \
             (`install` / `uninstall` / `on` / `off` also work)."
        ),
    }
}

/// Whether a bare argument is a sub-action rather than a workspace path.
///
/// `devp config hook <word>` is ambiguous by design — `<word>` is either the action or
/// the repository to apply it to — so both the argument router and [`parse_toggle`]
/// have to agree on which words are actions.
pub fn is_toggle_word(word: &str) -> bool {
    parse_toggle(word).is_ok() && !word.is_empty()
}

/// Resolve the workspace argument of `daemon` / `hook`, which is whatever was not
/// recognised as an action.
///
/// A word that is neither an action nor a directory is a mistyped action. Treating it
/// as a path would print `Daemon Status (enabel): Enabled for workspace` — a success
/// message about a repository that does not exist.
fn resolve_workspace(path: &str) -> Result<std::path::PathBuf> {
    let raw = Path::new(path);
    if !raw.is_dir() {
        bail!(
            "`{path}` is neither an action nor an existing directory.\n\
             Expected `enable`, `disable` or `status`, or a path to a repository."
        );
    }
    Ok(raw.canonicalize().unwrap_or_else(|_| raw.to_path_buf()))
}

/// Display a single config value.
pub fn run_get(key: &str) -> Result<()> {
    let registry = Registry::load()?;
    let setting = find_setting(key)?;
    println!("{key} = {}", (setting.get)(&registry.settings));
    Ok(())
}

/// Set a config value.
pub fn run_set(key: &str, value: &str) -> Result<()> {
    let mut registry = Registry::load()?;
    let setting = find_setting(key)?;
    (setting.set)(&mut registry.settings, value)?;
    registry.save()?;

    // The stored value, not the typed one: `devp config set auto_daemon yes` stores
    // `true`, and echoing "auto_daemon = yes" would describe a file that does not exist.
    output::print_success(&format!("{key} = {}", (setting.get)(&registry.settings)));

    // The one value that carries a caveat. A catalogue nobody has proofread is still
    // worth shipping — it is how the first speaker of that language finds the mistakes
    // — but they should hear it here rather than infer it from a wrong heading.
    if key == "language"
        && let Some(meta) = i18n::language(&registry.settings.language)
        && !meta.reviewed
    {
        output::print_info(&format!(
            "No native speaker has reviewed the {} translation yet. Corrections are welcome — see docs/TRANSLATIONS.md.",
            meta.english_name
        ));
    }
    Ok(())
}

/// Widest key name, so every value in `config show` lines up.
fn key_column_width() -> usize {
    SETTINGS.iter().map(|s| s.key.len()).max().unwrap_or(0)
}

/// Show all config values.
pub fn run_show() -> Result<()> {
    let registry = Registry::load()?;
    let width = key_column_width();

    output::print_header("dev-prune Global Configuration");
    for (category, settings) in settings_by_category() {
        output::print_section(category.title());
        for setting in settings {
            println!(
                "    {:<width$} = {}",
                setting.key,
                (setting.get)(&registry.settings)
            );
        }
    }

    // Not settings, and so not in a group with any: one is a count and the other is a
    // path, and neither is something `devp config set` will take.
    output::print_section("This machine");
    println!(
        "    {:<width$} = {}",
        "tracked_repos",
        registry.repo_count()
    );
    let reg_path = Registry::registry_path()
        .map(|p| output::clean_path(&p))
        .unwrap_or_else(|_| "unknown".to_string());
    println!("    {:<width$} = {reg_path}", "registry_file");

    // Until 1.10.0 the recommendations existed only on the first-run screen, so a
    // machine that had already been through it had no way left to find out that a
    // recommendation existed at all — let alone that one of them carries a caveat.
    print_recommendation_summary(&registry.settings);

    println!();
    output::print_info("Change any of these with `devp config set <key> <value>`.");
    output::print_info("Walk through them one at a time with `devp config wizard`.");

    Ok(())
}

/// Whether anybody asked for the configurator, or it opened on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// `devp config wizard`, typed on purpose.
    ByRequest,
    /// The first run after an install, or the first after an upgrade added a setting —
    /// the two times this takes a terminal in the middle of a command that asked for
    /// something else.
    OnItsOwn,
}

/// What to tell someone who did not ask to be here, or `None` when they did ask.
///
/// Two different situations and so two different sentences: a fresh install has never
/// seen any of this, while an upgrade has added a handful of keys to a list somebody
/// already went through. Both close on the same promise, which is the one the reader
/// actually wants — the command they typed is still going to run.
fn why_this_opened(opened: Opened) -> Option<String> {
    if opened == Opened::ByRequest {
        return None;
    }
    let new = settings_added_since_review().len();
    Some(if reviewed_version().is_none() || new == 0 {
        "You did not ask for this screen. dev-prune opens it once, on the first command \
         after it is installed, so that you see what its defaults do before they start \
         doing it. Whatever you typed runs as soon as you leave. It will not open by \
         itself again unless an upgrade adds a setting."
            .to_string()
    } else {
        format!(
            "You did not ask for this screen. This upgrade added {new} {}, and dev-prune \
             shows a new one once before its default goes on applying. Nothing else about \
             your configuration changed. Whatever you typed runs as soon as you leave.",
            output::plural(new, "setting", "settings"),
        )
    })
}

/// Put every global setting in front of the user, and let them change any of it.
///
/// Run by hand as `devp config wizard`, and once automatically — the first time a human
/// types a command on a fresh install, and again after an upgrade that added a setting
/// they have never been shown. Both are the moment a default starts applying to their
/// machine, and the only moment they can be told so before rather than after.
///
/// Two implementations, one meaning. [`run_wizard_tui`] is the full-screen one; the
/// line-by-line [`run_wizard_prompts`] runs wherever that cannot, which is less a
/// degraded mode than the only honest option on a pipe.
pub fn run_wizard(no_tui: bool, opened: Opened) -> Result<()> {
    if !no_tui && full_screen_is_usable() {
        return run_wizard_tui(opened);
    }
    run_wizard_prompts(opened)
}

/// Whether a full-screen view can be opened, and should be.
///
/// The terminal test answers "is there a screen to draw on". `DEV_PRUNE_NO_TUI` answers
/// the one it cannot: whether the thing holding that terminal is a person. An agent
/// driving `devp` through a pty passes every terminal check and will never press a key,
/// so it sets the variable and gets the prompts — or, better, skips this command
/// altogether for `devp config set`, which needs no interaction at all.
fn full_screen_is_usable() -> bool {
    use std::io::IsTerminal;
    if std::env::var_os(crate::constants::ENV_NO_TUI).is_some() {
        return false;
    }
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// The full-screen configurator: declaration, then every setting, then the summary.
fn run_wizard_tui(opened: Opened) -> Result<()> {
    use crate::tui::config_view::{ConfigRow, ConfigSession, Control, Outcome};

    let registry = Registry::load()?;
    let new_keys = settings_added_since_review();
    // What a machine that had never run this would hold, read through the same getters
    // rather than restated. A second spelling of every default is a second spelling free
    // to drift from `Settings::default()`, and this one is shown as fact.
    let fresh = Settings::default();

    // Grouped, not in the order of the table — the view draws a heading wherever the
    // category changes, so the order rows arrive in is the order they are read in.
    let rows: Vec<ConfigRow> = settings_by_category()
        .into_iter()
        .flat_map(|(category, settings)| {
            settings.into_iter().map(move |setting| (category, setting))
        })
        .map(|(category, setting)| {
            let value = (setting.get)(&registry.settings);
            ConfigRow {
                key: setting.key,
                category: category.title(),
                help: setting.help,
                plain: setting.plain,
                control: match setting.kind {
                    Kind::Toggle => Control::Toggle,
                    Kind::Choice => Control::Choice(i18n::choices()),
                    Kind::Number => Control::Number,
                    Kind::Adapters => Control::Adapters,
                    Kind::AdapterDays => Control::AdapterDays,
                    Kind::CacheCaps => Control::CacheCaps,
                },
                original: value.clone(),
                default: (setting.get)(&fresh),
                recommended: recommended_value(setting.key),
                cautious: recommendation(setting.key).is_some_and(|r| r.cautious),
                value,
                is_new: new_keys.contains(&setting.key),
            }
        })
        .collect();

    // The view validates through the real setters against a throwaway copy, so a value it
    // accepts is a value that will save, and the rules stay in exactly one place.
    let base = registry.settings.clone();
    let validate = move |key: &str, value: &str| -> std::result::Result<(), String> {
        let setting = find_setting(key).map_err(|e| e.to_string())?;
        let mut probe = base.clone();
        (setting.set)(&mut probe, value).map_err(|e| format!("{e}"))
    };

    let report = crate::commands::trust::build(&registry);
    let adapters = crate::adapters::all_adapter_names();
    let opt_in = crate::adapters::opt_in_adapter_names();
    // Identity, never a guess: the checklist offers a cache cap only where an adapter
    // and a cache go by the same name. See `ConfigSession::capped_adapters`.
    let capped: Vec<&'static str> = adapters
        .iter()
        .copied()
        .filter(|name| crate::commands::caches::is_cache_manager(name))
        .collect();

    let why = why_this_opened(opened);
    let outcome = crate::tui::config_view::run(ConfigSession {
        declaration: declaration_lines(&report),
        standing: NOTHING_DELETED_YET.to_string(),
        suggestions: first_run_suggestions(),
        rows,
        adapters: &adapters,
        opt_in_adapters: &opt_in,
        capped_adapters: &capped,
        groups: crate::adapters::ADAPTER_GROUPS,
        validate: &validate,
        title: "dev-prune configuration",
        uninvited: why.as_deref(),
    })?;

    match outcome {
        // Deliberately not marked reviewed here — the caller decides. The first run marks
        // it anyway, because being asked again on every command is worse than being asked
        // once and walking away; `devp config wizard` typed by hand changes nothing.
        Outcome::Cancelled => {
            output::print_info("Cancelled — nothing was changed.");
            Ok(())
        }
        Outcome::KeepAll => {
            mark_reviewed();
            output::print_success(
                "Keeping the current values. `devp config set <key> <value>` changes any.",
            );
            Ok(())
        }
        Outcome::Save(changed) => {
            save_settings_edits(changed.iter().map(|row| (row.key, row.value.as_str())))?;
            mark_reviewed();

            // Reprinted into the scrollback on purpose: the summary screen left with the
            // alternate screen, and what was just written to a config file should still be
            // readable after the view that wrote it has closed.
            output::print_header("Saved");
            let width = changed.iter().map(|r| r.key.len()).max().unwrap_or(0);
            for row in &changed {
                println!(
                    "  {:<width$} = {}  (was {})",
                    row.key, row.value, row.original
                );
            }
            println!();
            output::print_success(&format!(
                "{} {} saved. `devp config show` lists every setting.",
                changed.len(),
                output::plural(changed.len(), "change", "changes")
            ));
            Ok(())
        }
    }
}

/// The suggestions screen's contents — empty on every run but the first.
///
/// "First" is the same fact the walkthrough itself runs on: no review marker on disk
/// means this machine has never been shown the settings. Someone who types
/// `devp config wizard` a month later has already made these decisions once, and
/// re-suggesting them is how a suggestion turns into nagging.
///
/// The descriptions are read off the settings table rather than written again here.
/// Two copies of "what does `enable_cargo` do" is one copy free to drift, and the copy
/// on this screen is the one a brand-new user reads first.
/// The value [`RECOMMENDED`] suggests for a key, if it suggests one.
///
/// Unlike [`first_run_suggestions`] this answers on every run, not only the first. The
/// suggestions screen is shown once; the settings list is where somebody goes back to a
/// year later, and "what did the author think this should be" is a question that does
/// not expire with the screen that first asked it.
fn recommended_value(key: &str) -> Option<&'static str> {
    recommendation(key).map(|r| r.value)
}

/// The recommendation covering a setting, when one does.
fn recommendation(key: &str) -> Option<&'static Recommendation> {
    RECOMMENDED.iter().find(|r| r.key == key)
}

/// Whether a value already on the machine counts as having taken a recommendation.
///
/// The one place both `devp config show` and `devp config recommended` ask the
/// question, so a setting cannot be outstanding on one screen and already-set on the
/// other.
fn already_taken(rec: &Recommendation, current: &str) -> bool {
    match rec.taken {
        Some(is_taken) => is_taken(current),
        None => current == rec.value,
    }
}

/// Which recommendations a machine has not taken yet, in table order.
fn outstanding(settings: &Settings) -> Vec<&'static Recommendation> {
    RECOMMENDED
        .iter()
        .filter(|r| {
            find_setting(r.key)
                .map(|s| (s.get)(settings))
                .is_ok_and(|current| !already_taken(r, &current))
        })
        .collect()
}

/// The outstanding recommendations, in their two tiers, under the names both tiers are
/// known by everywhere else.
///
/// Prints nothing when there is nothing outstanding: a section whose entire content is
/// "nothing to do" is a section people learn to scroll past, and it would then be in the
/// way on every later reading of `devp config show`.
fn print_recommendation_summary(settings: &Settings) {
    let outstanding = outstanding(settings);
    if outstanding.is_empty() {
        return;
    }
    let width = key_column_width();

    let safe: Vec<_> = outstanding.iter().filter(|r| !r.cautious).collect();
    if !safe.is_empty() {
        output::print_section(SAFE_TIER);
        for r in &safe {
            println!("    {:<width$} = {}   {}", r.key, r.value, r.label);
        }
        println!();
        output::print_info(&format!(
            "`devp config recommended` sets {} {} in one command.",
            safe.len(),
            output::plural(safe.len(), "setting", "settings")
        ));
    }

    let cautious: Vec<_> = outstanding.iter().filter(|r| r.cautious).collect();
    if !cautious.is_empty() {
        output::print_section(CAUTIOUS_TIER);
        for r in &cautious {
            println!("    {:<width$} = {}   {}", r.key, r.value, r.label);
            println!("    {:<width$}   {}", "", r.why);
        }
        println!();
        output::print_info(
            "Not included above. `devp config recommended --with-cautious` includes it; \
             `devp config set <key> <value>` sets one on its own.",
        );
    }
}

/// Turn on everything the first run recommends, without the first run.
///
/// Reads the same table the configurator reads, so the one-command path and the
/// walkthrough cannot end up disagreeing about what "recommended" means.
///
/// The cautious tier is held back unless `--with-cautious` is typed. That is not the
/// same prohibition the configurator's `[a]` key is under: `[a]` would accept, on
/// somebody's behalf, the thing the screen had just told them to read about, whereas a
/// flag is the reading having happened. What it must not do is arrive by default.
///
/// It does not mark the settings as reviewed. This is a shortcut past the decision, not
/// the screen that puts the decision in front of somebody — so a machine configured
/// this way still gets the walkthrough it is owed.
pub fn run_recommended(with_cautious: bool) -> Result<()> {
    let mut registry = Registry::load()?;
    let width = key_column_width();

    output::print_header("dev-prune recommended settings");

    let mut applied: Vec<(&'static str, String, &'static str)> = Vec::new();
    let mut already: Vec<&'static Recommendation> = Vec::new();
    let mut held_back: Vec<&'static Recommendation> = Vec::new();

    for rec in RECOMMENDED {
        let setting = find_setting(rec.key)?;
        let current = (setting.get)(&registry.settings);
        if already_taken(rec, &current) {
            already.push(rec);
        } else if rec.cautious && !with_cautious {
            held_back.push(rec);
        } else {
            (setting.set)(&mut registry.settings, rec.value)?;
            applied.push((rec.key, current, rec.value));
        }
    }

    if !applied.is_empty() {
        registry.save()?;
        output::print_section("Turned on");
        for (key, from, to) in &applied {
            println!("    {:<width$}   {from} → {to}", key);
        }
    }
    if !already.is_empty() {
        output::print_section("Already set");
        for rec in &already {
            println!("    {:<width$}   {}", rec.key, rec.label);
        }
    }
    if !held_back.is_empty() {
        output::print_section(CAUTIOUS_TIER);
        for rec in &held_back {
            println!("    {:<width$} = {}   {}", rec.key, rec.value, rec.label);
            println!("    {:<width$}   {}", "", rec.why);
        }
        println!();
        output::print_info(
            "Left alone. `devp config recommended --with-cautious` includes it; \
             `devp config set <key> <value>` sets one on its own.",
        );
    }

    println!();
    if applied.is_empty() {
        output::print_success(
            "Nothing changed — everything recommended without a caveat is already set.",
        );
    } else {
        output::print_success(&format!(
            "{} {} changed. `devp config show` lists them all.",
            applied.len(),
            output::plural(applied.len(), "setting", "settings")
        ));
    }
    Ok(())
}

fn first_run_suggestions() -> Vec<crate::tui::config_view::Suggestion> {
    use crate::tui::config_view::Suggestion;

    if reviewed_version().is_some() {
        return Vec::new();
    }
    RECOMMENDED
        .iter()
        .filter_map(|r| {
            let setting = find_setting(r.key).ok()?;
            Some(Suggestion {
                key: r.key,
                label: r.label,
                help: setting.help,
                plain: setting.plain,
                why: r.why,
                value: r.value,
                cautious: r.cautious,
            })
        })
        .collect()
}

/// What is true at the moment the configurator opens, and stays true while it is open.
const NOTHING_DELETED_YET: &str =
    "Nothing has been deleted, and nothing will be until a lockfile proves it comes back.";

/// Who wrote this, and where a copy of it legitimately comes from.
///
/// Everything else on the declaration screen is a promise about what dev-prune will not
/// do, and a promise is worth what the thing making it is: the screen listed seven
/// guarantees without ever saying whose binary was guaranteeing them. This is that
/// block, and it is the one place on the screen a reader can act on before trusting the
/// rest — by checking the download against a name and a URL they can verify.
///
/// Read from `constants` rather than written out here, because `devp --version` reads
/// the same values: a stray copy of the executable and the screen that vouches for it
/// must not be able to disagree about who built it.
///
/// Every channel listed is one dev-prune is actually published to today. WinGet is
/// deliberately absent until it is, because a provenance list that names a channel
/// nobody publishes to teaches people to trust a name instead of a source, which is the
/// exact habit this block exists to prevent.
fn provenance_rows() -> Vec<(&'static str, String)> {
    // Trimmed of the scheme so the longest line still fits an 80-column terminal beside
    // a 26-cell label column; nothing here is a link to click.
    let url = |u: &str| u.trim_start_matches("https://").to_string();
    vec![
        (
            "What you are running",
            format!(
                "{} v{}",
                crate::constants::APP_NAME,
                crate::constants::VERSION
            ),
        ),
        (
            "Written by",
            format!("{}, under Apache-2.0", crate::constants::AUTHOR),
        ),
        ("Source code", url(crate::constants::REPO_URL)),
        (
            "Official downloads",
            format!("{} · GitHub releases", url(crate::constants::HOMEPAGE_URL)),
        ),
        (
            "Package registries",
            "crates.io · PyPI · npm, all named dev-prune".to_string(),
        ),
        (
            "Editor extension",
            "VS Code Marketplace · Open VSX".to_string(),
        ),
        (
            "Any other source",
            "is not a copy the author published".to_string(),
        ),
    ]
}

/// The declaration screen's contents: `devp trust`, shown before rather than after.
///
/// Read off the same report that command prints rather than written out again here. A
/// second copy of these promises is a second copy free to drift, and the copy a new user
/// reads first is the worst one to have drift.
fn declaration_lines(
    report: &crate::commands::trust::TrustReport,
) -> Vec<crate::tui::config_view::DeclarationLine> {
    use crate::commands::trust::{TrustRow, Verdict};
    use crate::tui::config_view::DeclarationLine;

    let heading = |text: &str| DeclarationLine {
        mark: '#',
        subject: text.to_string(),
        state: String::new(),
    };
    let row = |r: &TrustRow| DeclarationLine {
        mark: match r.verdict {
            Verdict::Guaranteed | Verdict::Safe => '+',
            Verdict::Widened => '!',
            Verdict::Neutral => ' ',
        },
        subject: r.subject.to_string(),
        state: r.state.clone(),
    };

    let mut lines = vec![heading("What this is, and where it came from")];
    lines.extend(
        provenance_rows()
            .into_iter()
            .map(|(subject, state)| DeclarationLine {
                mark: ' ',
                subject: subject.to_string(),
                state,
            }),
    );
    lines.push(heading(""));
    lines.push(heading("Guaranteed by the code"));
    lines.extend(report.guarantees.iter().map(&row));
    lines.push(heading(""));
    lines.push(heading("On this machine"));
    lines.extend(report.machine.iter().map(&row));
    lines
}

/// Walk the global settings one line at a time, offering each current value.
///
/// Refuses without a terminal instead of hanging on a read that will never return.
fn run_wizard_prompts(opened: Opened) -> Result<()> {
    use std::io::{self, IsTerminal, Write};

    if !io::stdin().is_terminal() {
        bail!(
            "`devp config wizard` needs a terminal to ask questions on.\n\
             Use `devp config show` to read the settings and `devp config set <key> <value>` \
             to change one."
        );
    }

    let mut registry = Registry::load()?;
    let width = key_column_width();
    let new_keys = settings_added_since_review();
    let fresh = Settings::default();

    output::print_header("dev-prune configuration");
    // Before the list rather than after it: somebody who typed `devp caches` and got this
    // needs the reason at the top, where they are already looking, not under thirty keys.
    if let Some(why) = why_this_opened(opened) {
        output::print_warning(&why);
        println!();
    }
    output::print_section("What this is, and where it came from");
    for (subject, state) in provenance_rows() {
        println!("    {}  {state}", output::pad_display(subject, 22));
    }
    println!("    {}", crate::constants::LICENCE_NOTICE);
    println!();

    output::print_info("These are the defaults every run will use. Nothing has been changed yet.");
    println!();
    for (category, settings) in settings_by_category() {
        output::print_section(category.title());
        for setting in settings {
            // A setting that arrived in an upgrade has been applying its default since
            // the upgrade, so naming those is the whole reason this reopened.
            let badge = if new_keys.contains(&setting.key) {
                "   (new in this version)"
            } else {
                ""
            };
            println!(
                "    {:<width$} = {}{badge}",
                setting.key,
                (setting.get)(&registry.settings)
            );
            println!("    {:<width$}   {}", "", setting.help);
            // Both lines here too. This path is what a pipe, a narrow terminal and
            // `DEV_PRUNE_NO_TUI` all get, and it is no place to be the terse one.
            println!("    {:<width$}   {}", "", setting.plain);
            // Same two facts the full-screen detail pane carries. The short path is
            // allowed to be shorter; it is not allowed to be the one that leaves out
            // what a fresh install would have done.
            let mut facts = format!("default {}", (setting.get)(&fresh));
            // Which tier, not just "recommended". The cautious one is the whole reason
            // the distinction exists, and a line that prints both the same way is the
            // line that loses it.
            if let Some(rec) = recommendation(setting.key) {
                facts.push_str(&format!(
                    "  ·  recommended {} ({}, not required)",
                    rec.value,
                    if rec.cautious {
                        "read the note below first"
                    } else {
                        "suggested"
                    }
                ));
            }
            println!("    {:<width$}   {facts}", "");
            if let Some(rec) = recommendation(setting.key).filter(|r| r.cautious) {
                println!("    {:<width$}   {}", "", rec.why);
            }
        }
    }
    println!();

    print_recommendation_summary(&registry.settings);
    println!();

    // Two presses here even though the full-screen configurator now finishes on one:
    // that one walks a list and ends at a summary, while this line is the only thing
    // standing between a held-down Enter and "reviewed". One press is what somebody
    // presses to get past a screen they have stopped reading.
    if confirmed_twice("Press Enter twice to keep all of these, or type anything to change them: ")?
    {
        mark_reviewed();
        output::print_success("Keeping the defaults. `devp config set <key> <value>` changes any.");
        return Ok(());
    }

    println!();
    output::print_info("Enter a new value, or press Enter to keep the one shown.");
    println!();

    let mut edits: Vec<(&'static str, String, String)> = Vec::new();
    let mut input_ended = false;
    for setting in SETTINGS {
        if input_ended {
            break;
        }
        let current = (setting.get)(&registry.settings);
        loop {
            print!("  {} [{current}]: ", setting.key);
            io::stdout().flush()?;
            let mut line = String::new();
            // EOF mid-way — a closed pipe or Ctrl-D — keeps what has been answered so far
            // rather than looping forever on an empty read.
            if io::stdin().read_line(&mut line)? == 0 {
                println!();
                input_ended = true;
                break;
            }
            let typed = line.trim();
            if typed.is_empty() {
                break;
            }
            match (setting.set)(&mut registry.settings, typed) {
                Ok(()) => {
                    // Read back rather than recording what was typed: a setter is
                    // allowed to normalise, and a summary that quotes the keystrokes
                    // would then describe something other than what gets written.
                    let now = (setting.get)(&registry.settings);
                    if now != current {
                        edits.push((setting.key, current.clone(), now));
                    }
                    break;
                }
                // Re-asked rather than aborted: losing the eight answers already given
                // because the ninth was a typo is not a reasonable trade.
                Err(e) => output::print_error(&format!("{e}")),
            }
        }
    }

    println!();
    if edits.is_empty() {
        // EOF is not a review. Ctrl-D at the "keep all" gate correctly refused to
        // confirm, but then fell through to here — where the empty walkthrough set
        // the marker, so closing the input counted as having read every setting.
        if input_ended {
            output::print_info("Input ended — nothing was changed.");
            return Ok(());
        }
        mark_reviewed();
        output::print_success("Nothing changed — the defaults are in place.");
        return Ok(());
    }

    // The last screen of the full-screen configurator, on one line per change: what is
    // about to be written, before it is written.
    output::print_section("About to be saved");
    for (key, from, to) in &edits {
        println!("    {:<width$}   {from} → {to}", key);
    }
    println!();
    if !confirmed_twice("Press Enter twice to save, or type anything to abandon: ")? {
        output::print_info("Nothing was written.");
        return Ok(());
    }

    save_settings_edits(edits.iter().map(|(key, _, to)| (*key, to.as_str())))?;
    mark_reviewed();
    let changed = edits.len();
    println!();
    output::print_success(&format!(
        "Saved {changed} {}. `devp config show` lists them all.",
        output::plural(changed, "change", "changes")
    ));
    Ok(())
}

/// Two empty lines to say yes: in line mode a single Enter is what people press to
/// dismiss a prompt they have stopped reading, so keeping-or-saving costs two. (The
/// full-screen configurator does not need this — its Enter walk always lands on the
/// summary before anything is written.)
///
/// Anything typed is a no, and so is EOF: a closed pipe must not be able to answer a
/// confirmation, and the only way to be sure of that is to treat the absence of an
/// answer as one.
fn confirmed_twice(prompt: &str) -> Result<bool> {
    use std::io::{self, Write};

    for pass in 0..2 {
        print!(
            "{}",
            if pass == 0 {
                prompt
            } else {
                "Press Enter once more to confirm: "
            }
        );
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            println!();
            return Ok(false);
        }
        if !line.trim().is_empty() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Marker recording that the settings have been put in front of the user once.
const REVIEW_MARKER: &str = "config-reviewed";

/// Whether the walkthrough is owed: on a fresh install, or after an upgrade that added a
/// setting this machine has never been shown.
///
/// An upgrade does not re-ask about settings already confirmed — being made to reconfirm
/// `idle_days` every release is a nuisance, and a nuisance is something people learn to
/// dismiss without reading. It reopens only when something is genuinely new, and then
/// says which. A `devp uninstall --purge` removes the config directory and with it this
/// marker, which is what makes a real reinstall ask about everything again.
pub fn config_review_is_due() -> bool {
    let Ok(dir) = Registry::config_dir() else {
        return false;
    };
    if !dir.join(REVIEW_MARKER).exists() {
        return true;
    }
    !settings_added_since_review().is_empty()
}

/// The release recorded the last time the settings were put in front of the user.
fn reviewed_version() -> Option<String> {
    let dir = Registry::config_dir().ok()?;
    let recorded = std::fs::read_to_string(dir.join(REVIEW_MARKER)).ok()?;
    let recorded = recorded.trim().to_string();
    (!recorded.is_empty()).then_some(recorded)
}

/// The settings that did not exist the last time this machine was asked.
///
/// Derived from each setting's own `since` rather than from a hand-kept "new in this
/// version" list, because that list is one more thing to forget when adding a setting and
/// its failure mode is silent: a new default starts applying and nothing ever says so.
///
/// Empty when the marker is missing or unreadable — that is the fresh-install case, where
/// every setting is new and [`config_review_is_due`] has already said so.
pub fn settings_added_since_review() -> Vec<&'static str> {
    let Some(reviewed) = reviewed_version() else {
        return Vec::new();
    };
    SETTINGS
        .iter()
        .filter(|s| {
            crate::commands::update::compare_versions(s.since, &reviewed)
                == Some(std::cmp::Ordering::Greater)
        })
        .map(|s| s.key)
        .collect()
}

fn mark_reviewed() {
    if let Ok(dir) = Registry::config_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(REVIEW_MARKER), crate::constants::VERSION);
    }
}

/// Suppress the first-run walkthrough without running it.
///
/// For the paths that must not stop to ask: the Git hook, the scheduler, and anything
/// with no terminal attached.
pub fn skip_config_review() {
    mark_reviewed();
}

/// Global audit pass for all registered repos.
pub fn run_global_update() -> Result<()> {
    output::print_header("dev-prune Global Configuration Audit & Sync");

    let registry = Registry::load()?;
    let mut total_audited = 0;
    let mut errors_found = 0;

    for repo_path in registry.repositories.keys() {
        let clean = output::clean_path(repo_path);

        // A registered path that is gone — deleted, on an unplugged drive — is not a
        // config error, and writing a fresh `.devprune.json` at it would either fail or
        // conjure a directory where the repository used to be.
        if !repo_path.exists() {
            output::print_warning(&format!(
                "Skipped {clean} — the path no longer exists. `devp unlink --missing` \
                 clears such entries."
            ));
            continue;
        }
        total_audited += 1;

        match PerRepoConfig::load_personal_for_write(repo_path) {
            Ok(Some(cfg)) => {
                if let Err(e) = cfg.save_to_repo(repo_path) {
                    output::print_error(&format!("Failed to write config for {clean}: {e}"));
                    errors_found += 1;
                } else {
                    output::print_success(&format!("Audited & synced config for {clean}"));
                }
            }
            Ok(None) => {
                // No file means the global defaults apply, which is a valid state, not a
                // gap to fill. Writing one here would drop an untracked file into every
                // registered repository in a single command.
                output::print_info(&format!(
                    "{clean} has no .devprune.json — global defaults apply."
                ));
            }
            Err(err_msg) => {
                errors_found += 1;
                output::print_error(&format!("Syntax/Schema Error in {clean}:"));
                for line in err_msg.lines() {
                    eprintln!("    {line}");
                }
                output::print_info(&format!(
                    "Hint: fix the syntax by hand, or run `devp config {clean} --update` to \
                     replace the file with a valid default."
                ));
            }
        }
    }

    if errors_found > 0 {
        // Non-zero, so a CI step or a shell `&&` chain notices. An audit that found
        // broken config files has not succeeded, however calmly it says so.
        anyhow::bail!(
            "Audit complete: {total_audited} repos checked, {errors_found} could not be read \
             or written."
        );
    }
    output::print_success(&format!(
        "Audit complete: All {total_audited} registered repositories are healthy & synced!"
    ));

    Ok(())
}

/// Inspect or create per-repository configuration.
///
/// `shared` addresses `project.devprune.json`, the half meant to be committed, rather than
/// the personal `.devprune.json` that gets excluded from git the moment it is written.
pub fn run_path_config(path_str: &str, force_update: bool, team: bool) -> Result<()> {
    let raw_path = Path::new(path_str);

    let path = if raw_path.exists() {
        raw_path
            .canonicalize()
            .unwrap_or_else(|_| raw_path.to_path_buf())
    } else {
        raw_path.to_path_buf()
    };

    let clean = output::clean_path(&path);

    if !path.exists() {
        bail!("Path does not exist: {clean}");
    }

    if !crate::scanner::is_git_repo(&path) {
        // The old text said "Initializing Git repo first..." and then did no such thing.
        bail!(
            "`{clean}` is not a Git repository.\n  \
             Run `git init` there first, then `devp config {clean}` again."
        );
    }

    let mut registry = Registry::load()?;
    if !registry.repositories.contains_key(&path) {
        output::print_info(&format!(
            "{clean} is not yet registered with dev-prune. Registering now..."
        ));
        registry.add_repo(path.clone());
        registry.save()?;
    }

    let name = if team {
        crate::constants::PROJECT_REPO_CONFIG_FILE
    } else {
        crate::constants::PER_REPO_CONFIG_FILE
    };
    let cfg_file = path.join(name);

    if cfg_file.exists() && !force_update {
        output::print_header(&format!("dev-prune Per-Repo Config for {clean}"));
        match crate::config::RepoConfigLayers::load(&path) {
            Ok(layers) => {
                let addressed = if team {
                    layers.project_config()
                } else {
                    layers.personal_config()
                };
                println!("{}", serde_json::to_string_pretty(&addressed)?);
                output::print_info(&format!("File location: {name}"));
                print_layer_provenance(&layers);
            }
            Err(err_msg) => {
                output::print_error(&format!("Invalid configuration in {clean}:"));
                for line in err_msg.lines() {
                    eprintln!("    {line}");
                }
                // Non-zero: the file this command was asked to show could not be read,
                // and the same file is what every prune of this repo will trip over.
                anyhow::bail!(
                    "Run `devp config {clean} --update` to reset this file back to defaults \
                     (your current overrides in it are discarded)."
                );
            }
        }
    } else {
        output::print_info(&format!("Initializing {name} for {clean}..."));
        if team {
            crate::config::write_project_starter(&path)?;
        } else {
            PerRepoConfig::default().save_to_repo(&path)?;
        }
        output::print_success(&format!("Created {name} in {clean}"));
        if team {
            output::print_info(
                "It starts empty on purpose: every key it names overrules \
                 `.devprune.json`, so it should only name the ones your team decides.",
            );
            output::print_info(
                "`prunable.directories` is the exception — the two files' lists add \
                 up, so naming one here never discards somebody's own.",
            );
            output::print_info(
                "Commit it. Unlike `.devprune.json`, this file is not added to \
                 `.git/info/exclude` — being shared is the whole reason it exists.",
            );
        }
    }

    Ok(())
}

/// Say which of the two files each effective setting came from.
///
/// Only worth printing when both exist. With one file the answer is the file you are
/// already looking at, and a table restating that is noise; with two, "which one won" is
/// the only question the two files cannot answer between them. Printed rather than
/// mirrored into `.devprune.json`, because a copied value is a second copy free to drift
/// from the first and then be believed.
fn print_layer_provenance(layers: &crate::config::RepoConfigLayers) {
    if layers.project_config().is_none() || layers.personal_config().is_none() {
        return;
    }
    output::print_section("Effective values");
    for (key, value, source) in layers.rows() {
        println!(
            "  {}  {}  {}",
            output::pad_display(key, 20),
            output::pad_display(&value, 14),
            source.label()
        );
    }
}

/// Load a workspace's `.devprune.json` for a toggle that is about to write it back.
///
/// Refuses a file that does not parse, rather than starting from the defaults. Starting
/// from the defaults meant `devp config <repo> daemon off` wrote a fresh file straight
/// over the broken one, so a single typo cost the user every other override in it.
fn load_workspace_config_for_write(repo_path: &Path) -> Result<PerRepoConfig> {
    match PerRepoConfig::load_personal_for_write(repo_path) {
        Ok(Some(cfg)) => Ok(cfg),
        Ok(None) => Ok(PerRepoConfig::default()),
        Err(e) => bail!(
            "{e}\n  \
             Fix that file, or run `devp config {} --update` to reset it back to defaults \
             (your current overrides in it are discarded).",
            output::clean_path(repo_path)
        ),
    }
}

/// Toggle or status check for background daemon (global or local workspace).
pub fn run_daemon_toggle(path: Option<&str>, action: &str) -> Result<()> {
    if let Some(p) = path {
        let repo_path = resolve_workspace(p)?;
        let mut cfg = load_workspace_config_for_write(&repo_path)?;
        match parse_toggle(action)? {
            Toggle::Enable => {
                cfg.disable_daemon = false;
                cfg.save_to_repo(&repo_path)?;
                output::print_success(&format!(
                    "Enabled background daemon for workspace: {}",
                    output::clean_path(&repo_path)
                ));
            }
            Toggle::Disable => {
                cfg.disable_daemon = true;
                cfg.save_to_repo(&repo_path)?;
                output::print_success(&format!(
                    "Disabled background daemon for workspace: {}",
                    output::clean_path(&repo_path)
                ));
            }
            Toggle::Status => {
                let st = if cfg.disable_daemon {
                    "Disabled for workspace"
                } else {
                    "Enabled for workspace"
                };
                output::print_info(&format!(
                    "Daemon Status ({}): {}",
                    output::clean_path(&repo_path),
                    st
                ));
            }
        }
    } else {
        match parse_toggle(action)? {
            Toggle::Enable => crate::commands::daemon::run_install()?,
            Toggle::Disable => crate::commands::daemon::run_uninstall()?,
            Toggle::Status => crate::commands::daemon::run_status()?,
        }
    }
    Ok(())
}

/// Toggle or status check for background Git hooks (global or local workspace).
pub fn run_hook_toggle(path: Option<&str>, action: &str, chain: bool) -> Result<()> {
    if let Some(p) = path {
        if chain {
            bail!(
                "`--chain` changes the single global `core.hooksPath`, so it has no \
                 per-workspace form. Drop the path: `devp hook install --chain`."
            );
        }
        let repo_path = resolve_workspace(p)?;
        let mut cfg = load_workspace_config_for_write(&repo_path)?;
        match parse_toggle(action)? {
            Toggle::Enable => {
                cfg.disable_hooks = false;
                cfg.save_to_repo(&repo_path)?;
                output::print_success(&format!(
                    "Enabled background Git hooks for workspace: {}",
                    output::clean_path(&repo_path)
                ));
            }
            Toggle::Disable => {
                cfg.disable_hooks = true;
                cfg.save_to_repo(&repo_path)?;
                output::print_success(&format!(
                    "Disabled background Git hooks for workspace: {}",
                    output::clean_path(&repo_path)
                ));
            }
            Toggle::Status => {
                let st = if cfg.disable_hooks {
                    "Disabled for workspace"
                } else {
                    "Enabled for workspace"
                };
                output::print_info(&format!(
                    "Git Hook Status ({}): {}",
                    output::clean_path(&repo_path),
                    st
                ));
            }
        }
    } else {
        match parse_toggle(action)? {
            Toggle::Enable => crate::commands::hook::run_install(chain)?,
            Toggle::Disable => crate::commands::hook::run_uninstall()?,
            Toggle::Status => crate::commands::hook::run_status()?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_synonyms_all_resolve_to_enable() {
        for word in ["enable", "install", "on", "INSTALL", "On"] {
            assert_eq!(parse_toggle(word).unwrap(), Toggle::Enable, "{word}");
        }
    }

    #[test]
    fn disable_synonyms_all_resolve_to_disable() {
        for word in ["disable", "uninstall", "remove", "off", "Uninstall"] {
            assert_eq!(parse_toggle(word).unwrap(), Toggle::Disable, "{word}");
        }
    }

    #[test]
    fn status_is_the_default_and_is_also_spellable() {
        for word in ["", "status", "show"] {
            assert_eq!(parse_toggle(word).unwrap(), Toggle::Status, "{word}");
        }
    }

    #[test]
    fn a_typo_is_an_error_rather_than_a_silent_status_report() {
        // `devp config daemon enabel` must not print status and exit 0 — that reads as
        // success while the daemon stays uninstalled.
        let err = parse_toggle("enabel").unwrap_err().to_string();
        assert!(err.contains("enabel"), "{err}");
        assert!(err.contains("enable"), "{err}");
    }

    #[test]
    fn a_workspace_toggle_refuses_to_write_over_a_broken_config() {
        // The toggle rewrites the whole file. Starting from the defaults on a file it
        // could not read would silently discard every override the user had put in it.
        let tmp = tempfile::TempDir::new().unwrap();
        let broken = r#"{ "project_name": "api", "override_idle_days": 90, }"#;
        std::fs::write(
            tmp.path().join(crate::constants::PER_REPO_CONFIG_FILE),
            broken,
        )
        .unwrap();

        let err = load_workspace_config_for_write(tmp.path())
            .unwrap_err()
            .to_string();
        assert!(err.contains("Syntax error"), "{err}");
        assert!(err.contains("--update"), "{err}");

        // Untouched, so the user still has their 90 days to recover.
        let on_disk =
            std::fs::read_to_string(tmp.path().join(crate::constants::PER_REPO_CONFIG_FILE))
                .unwrap();
        assert_eq!(on_disk, broken);
    }

    #[test]
    fn the_suggested_cap_is_the_constant_it_claims_to_be() {
        // The suggestion table holds strings and the rest of the program holds a number,
        // so this is the only thing stopping the two from drifting — and `config
        // recommended` feeds this literal straight into the setter, so a value that does
        // not parse would be a runtime error on somebody else's machine.
        let caps = parse_cache_caps(crate::constants::RECOMMENDED_CACHE_CAP).unwrap();
        assert_eq!(
            caps,
            std::collections::BTreeMap::from([(
                crate::constants::CACHE_CAP_DEFAULT_KEY.to_string(),
                crate::constants::RECOMMENDED_CACHE_MAX_GB,
            )])
        );
    }

    #[test]
    fn a_ceiling_of_your_own_counts_as_having_taken_the_advice() {
        // The whole reason `taken` exists. Somebody who capped npm at 4 GiB decided
        // this; listing it as outstanding forever would be nagging, and `devp config
        // recommended` replacing their map with `default=10` would be worse — it would
        // throw away a number they chose.
        let rec = recommendation("cache_max_gb").expect("the cap is recommended");
        assert!(already_taken(rec, "npm=4"));
        assert!(already_taken(rec, crate::constants::RECOMMENDED_CACHE_CAP));
        assert!(!already_taken(rec, "(none)"));

        // And a toggle still answers the plain question.
        let toggle = recommendation("enable_cargo").expect("cargo is recommended");
        assert!(already_taken(toggle, "true"));
        assert!(!already_taken(toggle, "false"));
    }

    #[test]
    fn a_workspace_with_no_config_yet_starts_from_the_defaults() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(
            load_workspace_config_for_write(tmp.path()).unwrap(),
            PerRepoConfig::default()
        );
    }

    #[test]
    fn a_cache_cap_is_written_the_way_it_is_read_back() {
        let caps = parse_cache_caps("uv=10,npm=4").unwrap();
        assert_eq!(caps.get("uv"), Some(&10));
        assert_eq!(caps.get("npm"), Some(&4));
        // Sorted and normalised, so `config get` prints one spelling no matter which
        // order or casing the user typed.
        let settings = Settings {
            cache_max_gb: parse_cache_caps("UV = 10 , npm=4").unwrap(),
            ..Settings::default()
        };
        let printed = SETTINGS
            .iter()
            .find(|s| s.key == "cache_max_gb")
            .map(|s| (s.get)(&settings))
            .unwrap();
        assert_eq!(printed, "npm=4,uv=10");
        assert_eq!(parse_cache_caps(&printed).unwrap(), settings.cache_max_gb);
    }

    #[test]
    fn clearing_the_caps_is_spelled_the_way_the_getter_prints_an_empty_map() {
        for blank in ["", "-", "none", "(none)", "NONE"] {
            assert!(
                parse_cache_caps(blank).unwrap().is_empty(),
                "`{blank}` should clear every cap"
            );
        }
    }

    #[test]
    fn a_cap_on_something_that_is_not_a_cache_is_refused_with_the_list() {
        // `venv`, `terraform` and `dart` are adapters with no cache of their own, and
        // accepting a cap for one would store a setting nothing ever reads.
        let err = parse_cache_caps("venv=10").unwrap_err().to_string();
        assert!(err.contains("venv"), "{err}");
        assert!(err.contains("npm"), "the error lists what is valid: {err}");
    }

    #[test]
    fn a_cap_has_to_be_a_whole_number_of_gibibytes() {
        for bad in ["uv=10.5", "uv=ten", "uv=-1", "uv="] {
            assert!(parse_cache_caps(bad).is_err(), "`{bad}` was accepted");
        }
        // A bare name is not a cap, and guessing a default for it would be a number the
        // user never chose.
        assert!(parse_cache_caps("uv").is_err());
    }

    #[test]
    fn a_cap_of_zero_is_refused_rather_than_stored() {
        // Zero marks the cache over-size the moment it exists, which is almost always a
        // typo for clearing the cap.
        let err = parse_cache_caps("uv=0").unwrap_err().to_string();
        assert!(
            err.contains("`-`"),
            "the error names the way to clear it: {err}"
        );
    }

    #[test]
    fn every_setting_round_trips_through_its_own_getter() {
        // The table is what `get`, `set`, `show` and the wizard all read, so a getter
        // that reports a different field than its setter writes would be invisible in
        // every one of them at once.
        let mut settings = Settings::default();
        for setting in SETTINGS {
            let before = (setting.get)(&settings);
            let probe = match setting.kind {
                Kind::Toggle => if before == "true" { "false" } else { "true" }.to_string(),
                // A number every numeric setting accepts: above every minimum, below
                // `scan_depth`'s ceiling.
                Kind::Number => "7".to_string(),
                // A real adapter name, so the round trip also proves the list prints
                // back in the spelling `config set` takes.
                Kind::Adapters => "cargo".to_string(),
                // Same, with a window attached: proves the map prints back in the
                // `name=days` spelling `config set` parses.
                Kind::AdapterDays => "cargo=45".to_string(),
                // A name that is a cache manager, which `cargo` also happens to be —
                // spelled out separately because the two lists are validated apart.
                Kind::CacheCaps => "cargo=10".to_string(),
                // A language every catalogue ships and the default is not, so the probe
                // is a real change rather than a write that happens to match.
                Kind::Choice => "hi".to_string(),
            };
            (setting.set)(&mut settings, &probe)
                .unwrap_or_else(|e| panic!("{} rejected `{probe}`: {e}", setting.key));
            assert_eq!(
                (setting.get)(&settings),
                probe,
                "{} reads back a different field than it writes",
                setting.key
            );
        }
    }

    #[test]
    fn every_setting_is_documented_and_uniquely_named() {
        let mut seen = std::collections::HashSet::new();
        for setting in SETTINGS {
            assert!(seen.insert(setting.key), "duplicate key {}", setting.key);
            assert!(!setting.help.is_empty(), "{} has no help", setting.key);
            assert!(
                !setting.plain.is_empty(),
                "{} has no plain text",
                setting.key
            );
            // The wizard prints both under the key; sentences keep that readable.
            assert!(
                setting.help.ends_with('.'),
                "{} help should read as a sentence",
                setting.key
            );
            assert!(
                setting.plain.ends_with('.'),
                "{} plain text should read as a sentence",
                setting.key
            );
            // Two ways of saying it, not the same way twice: a `plain` line that repeats
            // `help` costs a screen row and teaches nobody anything.
            assert_ne!(
                setting.plain, setting.help,
                "{} says the same thing twice",
                setting.key
            );
        }
    }

    #[test]
    fn the_settings_table_covers_every_field_of_settings() {
        // Serialising `Settings` names every field, so a field added without a table
        // entry — unsettable, unshown, never asked about — fails here rather than in
        // a bug report.
        let json = serde_json::to_value(Settings::default()).unwrap();
        let fields: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        for field in fields {
            assert!(
                SETTINGS.iter().any(|s| s.key == field),
                "`{field}` is a setting with no entry in SETTINGS, so `devp config set \
                 {field}` cannot reach it"
            );
        }
    }

    #[test]
    fn a_rejected_value_leaves_the_previous_one_in_place() {
        let mut settings = Settings::default();
        assert!((find_setting("scan_depth").unwrap().set)(&mut settings, "0").is_err());
        assert_eq!(settings.scan_depth, Settings::default().scan_depth);

        assert!((find_setting("command_timeout_secs").unwrap().set)(&mut settings, "0").is_err());
        assert!((find_setting("check_interval_days").unwrap().set)(&mut settings, "0").is_err());
        assert!(
            (find_setting("update_check_interval_days").unwrap().set)(&mut settings, "0").is_err()
        );
    }

    #[test]
    fn booleans_accept_the_words_people_actually_type() {
        assert!(parse_bool("k", "yes").unwrap());
        assert!(parse_bool("k", "ON").unwrap());
        assert!(!parse_bool("k", "0").unwrap());
        assert!(parse_bool("k", "maybe").is_err());
    }

    #[test]
    fn an_unknown_key_lists_the_ones_that_exist() {
        let err = match find_setting("idel_days") {
            Ok(_) => panic!("`idel_days` is not a setting"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("idle_days"), "{err}");
    }

    #[test]
    fn a_path_is_never_mistaken_for_an_action() {
        // The router uses this to decide whether a lone argument is a path or an action.
        assert!(!is_toggle_word("~/Code/my-repo"));
        assert!(!is_toggle_word("."));
        assert!(!is_toggle_word(""));
        assert!(is_toggle_word("install"));
    }
}
