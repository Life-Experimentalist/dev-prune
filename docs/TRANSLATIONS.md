# 🌐 Translations

dev-prune prints its own headings and summary lines in twelve languages. Adding a
thirteenth is one JSON file and one line of Rust, and fixing a wrong sentence in an
existing one is a one-line pull request that touches no code at all.

---

## What is translated, and what deliberately is not

The boundary is the whole design, so read it before you translate anything.

**Translated** — the chrome. Section headings, summary lines, and the group titles in
the configurator: the words that repeat on every run and that no script reads.

**Never translated**, in any language:

| Not translated | Why |
| :--- | :--- |
| Everything under `--json` | A document whose strings move with a setting is not a document. `devp run --json \| jq` has to mean the same thing on every machine. |
| Exit codes | `0` success, `1` failure, `2` usage error is a contract (see [CLI_REFERENCE.md](CLI_REFERENCE.md)). |
| Flag names, subcommand names, config keys | `devp config set idle_days 30` is the same command everywhere. A translated key is a key nobody can type from a tutorial. |
| Adapter and cache-manager names | `cargo`, `npm`, `uv` are the tools' own names, not English words. |
| Lockfile refusals and safety diagnoses | These end up pasted into bug reports. A refusal nobody upstream can read is a refusal that does not get fixed. |

So a translation can never change what a pipeline sees, and can never change what you
type. That is what makes it safe to ship a catalogue nobody has proofread yet.

---

## Where the files are

```
src/i18n/
  mod.rs            the loader, the merge, and the tests that guard both
  locales/
    en.json         English — the source of truth
    zh.json          简体中文 — Simplified Chinese
    hi.json  te.json  ta.json  kn.json  ml.json
    bn.json  mr.json  gu.json  pa.json  sa.json
```

Every catalogue is `include_str!`-ed into the binary, so there is nothing to install
alongside `devp` and nothing to go missing at runtime.

---

## The catalogue format

A catalogue is a flat JSON object. One `_meta` block that describes the file, then one
key per string:

```json
{
  "_meta": {
    "code": "te",
    "english_name": "Telugu",
    "native_name": "తెలుగు",
    "reviewed": false
  },

  "run.header": "dev-prune run",
  "run.summary": "సారాంశం",
  "run.freed": "ఖాళీ చేయబడింది: {count} డైరెక్టరీలలో {size}"
}
```

- **`code`** is the value `devp config set language <code>` and `DEV_PRUNE_LANG` take.
  Use the shortest unambiguous IETF subtag — `te`, not `te-IN`. Where a language is
  written in more than one script, the bare subtag belongs to whichever form shipped
  first and the others carry the script: `zh` is Simplified because that is the
  catalogue that exists, and a Traditional one would be `zh-Hant` rather than a
  renaming of `zh`. Codes are a config value, so once a release has shipped one it is
  a contract like any other.
- **`native_name`** is what the configurator's language row shows. It is the only part
  of that row a reader can act on — `te` is not a word — so write the language's name
  *in* the language.
- **`reviewed`** is `false` until a native speaker has read the file through. See
  [below](#the-reviewed-flag).

### Placeholders

`{name}` placeholders are substituted by name, never by position, so you may put them
in whatever order your language wants:

```json
"run.freed.targeted": "{path} में {size} मुक्त किया"
```

The English string names every placeholder that exists for that key. Using one English
does not have will leave the literal `{whatever}` in the output; leaving one out just
drops that value.

### Missing and empty keys

English is loaded first and your language is overlaid on top of it, and an empty value
is skipped. So:

- A key you have not translated yet prints in English.
- A key you set to `""` prints in English.
- A key that does not exist in English is ignored entirely.

A half-finished catalogue degrades to English rather than to blanks. **You do not have
to translate every key to open a pull request.**

---

## Adding a language

Three steps, and the third is the one people forget.

**1. Copy `en.json` and translate it.**

```powershell
Copy-Item src\i18n\locales\en.json src\i18n\locales\<code>.json
```

Change the `_meta` block to your language, set `"reviewed": false`, and translate as
many values as you want to. Leave `"run.header"` as the literal `dev-prune run` — it is
the program's name, not a sentence.

**2. Add one line to `CATALOGUES` in `src/i18n/mod.rs`.**

```rust
const CATALOGUES: &[&str] = &[
    include_str!("locales/en.json"),
    // …
    include_str!("locales/<code>.json"),
];
```

That is the only Rust change. There is no second list to update: the code, both names
and the review status all come out of the file's own `_meta`, so a language cannot
half-exist because someone updated one list and not the other.

**3. Run the gate.**

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all --all-features
npm --prefix site run build
```

The tests in `src/i18n/mod.rs` will tell you if the file does not parse, if the code
collides with an existing one, if `_meta` leaves either name blank, or if the file
invents a key English does not have. That last one is the common mistake: a mistyped
key is invisible at runtime, because the merge simply ignores what English has no slot
for.

Then check it by eye:

```powershell
devp config set language <code>
devp caches
devp config show
```

---

## Fixing a string in an existing language

Edit the one value in `src/i18n/locales/<code>.json` and open a pull request. No Rust,
no build system, no new key. If you are a native speaker and you have read the whole
file, say so in the pull request and flip `_meta.reviewed` to `true` in the same
change.

---

## The `reviewed` flag

Every catalogue except English currently carries `"reviewed": false`.

That is not modesty. `devp config set language <code>` reads it, and prints a note when
the language you picked has not been reviewed:

```
✓ language = hi
→ No native speaker has reviewed the Hindi translation yet. Corrections are welcome —
  see docs/TRANSLATIONS.md.
```

An unreviewed translation is still worth shipping: it is how the first speaker of that
language finds the mistakes. But they should hear it from the tool at the moment they
choose it, rather than infer it from a heading that reads wrong.

**What earns `true`:** a native speaker read the file top to bottom — not just spot-
checked it — and either changed what was wrong or confirmed it was right.

---

## How the language is chosen at runtime

In order, first match wins:

1. **`DEV_PRUNE_LANG`** — for one command, or for a shell. `DEV_PRUNE_LANG=te devp run`.
2. **`devp config set language <code>`** — the durable per-user choice.
3. **`en`** — the default.

An unrecognised code falls back to English silently rather than failing: a machine
that cannot print a heading is not a machine that should refuse to prune. `devp config
set language` *does* reject an unknown code, with the list of what exists — that is the
place where a typo can still be fixed.

**The operating system's locale is deliberately not consulted.** A machine set to
French does not make `devp` start printing French headings at somebody who has been
reading the English ones for a year. The choice is made once, by a person, and it stays
made.

---

## The twelve catalogues

| Code | Language | Reviewed |
| :--- | :--- | :---: |
| `en` | English | ✅ |
| `hi` | हिन्दी — Hindi | ❌ |
| `te` | తెలుగు — Telugu | ❌ |
| `ta` | தமிழ் — Tamil | ❌ |
| `kn` | ಕನ್ನಡ — Kannada | ❌ |
| `ml` | മലയാളം — Malayalam | ❌ |
| `bn` | বাংলা — Bengali | ❌ |
| `mr` | मराठी — Marathi | ❌ |
| `gu` | ગુજરાતી — Gujarati | ❌ |
| `pa` | ਪੰਜਾਬੀ — Punjabi | ❌ |
| `sa` | संस्कृतम् — Sanskrit | ❌ |
| `zh` | 简体中文 — Chinese (Simplified) | ❌ |

`devp config set language <unknown>` prints the same list, so the binary is always the
current answer.

---

## Related

- [Contributing Guide](../CONTRIBUTING.md) — the gate, the style, and the pull request checklist.
- [CLI Reference](CLI_REFERENCE.md#8-devp-config-action) — the `language` key alongside every other setting.
- [Adding New Ecosystem Adapters](ADDING_ADAPTERS.md) — the other one-file-and-one-line extension point.
