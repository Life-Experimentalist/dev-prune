# dev-prune for JetBrains IDEs

A file-type micro-plugin: `.devprune.json` and `ignore.devprune.json` get the dev-prune
icon in the project tree of every JetBrains IDE (IntelliJ IDEA, WebStorm, PyCharm,
RustRover, GoLand, …). The files remain JSON to the IDE, so schema completion and
validation — from the `$schema` link the CLI writes, or from SchemaStore once the
[catalog entry](../../docs/IDE_INTEGRATION.md) is merged — are untouched.

## Building

Needs JDK 17+. No Gradle wrapper is checked in yet — generate one once with a local
Gradle (`gradle wrapper --gradle-version 8.10`) or open the project in IntelliJ and let
it do the same.

```bash
./gradlew buildPlugin
```

The distributable ZIP lands in `build/distributions/` and uploads as-is to the
[JetBrains Marketplace](https://plugins.jetbrains.com/). The full publish checklist is
in [docs/IDE_INTEGRATION.md](../../docs/IDE_INTEGRATION.md).

## Known gaps before first publish

- This scaffold has not been built in CI. `buildPlugin` downloads the IntelliJ platform
  (several GB) on first run, which is why it is not part of the repository gate.

Both icons — `META-INF/pluginIcon.svg` (marketplace listing) and
`icons/devprune.svg` (file tree) — are true vectors derived from
[`assets/devprune.svg`](../../assets/devprune.svg), so they render crisply at every
size and HiDPI factor.
