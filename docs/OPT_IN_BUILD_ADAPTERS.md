# The opt-in build adapters

Eighteen of dev-prune's thirty-five adapters ship switched off. This guide explains why
they are a separate class, what each one claims once you turn it on, and how to turn
them on, one at a time or from the full-screen picker.

The one-sentence version: everything else dev-prune deletes comes back by
**downloading**, and these directories come back by **recompiling**. A `node_modules/`
returns in the time it takes `npm ci` to run. A 40 GiB Unreal `DerivedDataCache/` or a
Rust `target/` returns in however long your machine takes to rebuild it, which on a big
project is measured in coffee breaks. That cost is real, so nobody finds one of these
directories deleted without having asked for it.

---

## What "opt-in" changes

A disabled opt-in adapter is invisible to the whole pass: `devp run` never verifies it,
never deletes under it, and never lists it in a report. Turning one on changes three
things:

1. Its directories start appearing in `devp run --dry-run` and count toward the pass.
2. It waits for the **longer** idle window: `max(build_idle_days, idle_days)`, 45 days
   by default instead of the usual 15. An actively built project keeps its warm build
   tree; only a repository that has sat untouched for weeks gives it up.
3. Every safety invariant still applies. The adapter must prove the directory is
   regenerable from what sits beside it (a lockfile, a build script, the editor's own
   project file) before anything is deleted, and there is no flag that skips that.

A dry run tells you when this group would have found something: if a repository uses
one of these tools and the adapter is off, `devp run --dry-run` ends with a
`Detected, but switched off` section naming the adapter, how many repositories use it,
and the exact `devp config set` command that turns it on. The same information is in
the `--json` document under the additive `recommendations` key. Once you have made
your choices and the reminder has become noise,
`devp config set recommendations false` silences it machine-wide; ignoring or
excepting a repository already silences it for that repository alone.

---

## The eighteen, and what each one claims

### Compilers and build tools

| Enable with `devp config set … true` | Detects | Deletes | Comes back via |
| --- | --- | --- | --- |
| `enable_cargo` | `Cargo.toml` + `Cargo.lock` | `target/` | next `cargo build` |
| `enable_gradle` | Gradle build files | `build/`, `.gradle/` | next `gradle build` |
| `enable_maven` | `pom.xml` | `target/` | next `mvn package` |
| `enable_sbt` | `build.sbt` carrying a setting, or `project/build.properties` pinning `sbt.version` | `target/`, `project/target/` | next `sbt compile` |
| `enable_swift` | `Package.swift` declaring a `Package(` | `.build/` | next `swift build` |
| `enable_dart` | `pubspec.yaml` + `pubspec.lock` | `.dart_tool/` | `dart pub get` plus the next build |
| `enable_mix_build` | Elixir Mix project | `_build/` | next `mix compile` |
| `enable_zig` | `build.zig` declaring a `pub fn build` | `.zig-cache/`, `zig-cache/`, `zig-out/` | next `zig build` |
| `enable_stack` | `stack.yaml` naming its `resolver:` or `snapshot:` | `.stack-work/` | next `stack build` |
| `enable_cabal` | `cabal.project` declaring its `packages` (a lone `*.cabal` file is never claimed) | `dist-newstyle/` | next `cabal build` |
| `enable_vcpkg` | `vcpkg.json` manifest | `vcpkg_installed/` | `vcpkg install` |
| `enable_cmake_build` | a tree holding a `CMakeCache.txt` that records a source directory inside this repository | that build tree | next `cmake --build` |
| `enable_dotnet_build` | `obj/project.assets.json` recording this directory's own project file | `bin/` (only when it holds nothing but `Debug`/`Release` output), `obj/` | next `dotnet build` |

The Mix entry is the group's clearest illustration of the download/recompile line: the
always-on `mix` adapter claims the downloaded `deps/`, and the opt-in `mix_build`
adapter claims the compiled `_build/` beside it. Same repository, two costs, two
switches.

### Game engines

Game engine import caches are the largest single directories most machines carry, and
they are also the ones an editor rebuilds entirely on its own: open the project again
and the engine re-imports every asset. dev-prune claims only the cache the editor
regenerates, never the assets or exported builds it is derived from.

| Enable with `devp config set … true` | Detects | Deletes | Never claimed |
| --- | --- | --- | --- |
| `enable_godot` | `project.godot` | `.godot/` (and `.import/` on Godot 3) | your scenes and scripts |
| `enable_unity` | `ProjectSettings/ProjectVersion.txt` carrying `m_EditorVersion:` | `Library/`, `Temp/` | `Assets/`, `ProjectSettings/`; refused entirely while `Temp/UnityLockfile` says the editor is open |
| `enable_unreal` | a `.uproject` with a `"FileVersion"` key | `DerivedDataCache/`, `Intermediate/` | `Saved/`, `Binaries/`, `Content/` |
| `enable_defold` | `game.project` carrying a `[project]` section | `build/` | `.internal/`, `assets/` |
| `enable_cocos` | a `package.json` that names Cocos Creator | `library/`, `temp/` | the exported `build/` output |

---

## Turning them on

One at a time, when the dry run recommends it:

```bash
devp config set enable_unity true
```

Or all of your choices at once from the interactive picker, which lists every switch
with its current state:

```bash
devp config
```

Give the group a longer or shorter wait than the default 45 days:

```bash
devp config set build_idle_days 60
```

One adapter can wait longer than the rest of the group via `adapter_idle_days`
(`devp config set adapter_idle_days cargo=90`); each entry only ever raises its own
adapter's wait. The full semantics of every key are in the
[CLI reference](CLI_REFERENCE.md).

To keep one repository out of it entirely, drop an `ignore.devprune.json` in its root
or set `"ignore": true` in its `.devprune.json`; both also silence the dry-run
recommendation for it.

---

## Why whole directories, and where cargo-sweep fits

Tools exist that trim a build tree instead of deleting it. The best known is
[cargo-sweep](https://github.com/holmgr/cargo-sweep), which removes the `target/`
artifacts the current toolchain no longer uses and keeps the warm incremental cache.
In a repository you build every day, that is the better tool, and the two are
complementary rather than competing.

dev-prune deliberately does not do surgical trimming, for the same reason it never
touches a repository you committed to yesterday: its scope is repositories idle past
`build_idle_days`, where every incremental artifact is already cold and the whole tree
is dead weight. Deleting part of a directory would also make the pass's arithmetic a
lie: the report says what it freed and `devp restore` knows what to bring back, and
both of those depend on the unit being the whole directory. The
[comparison pages on the site](https://devprune.vkrishna04.me/blog/vs/) hold the longer
version of this argument against cargo-sweep and its neighbours.
