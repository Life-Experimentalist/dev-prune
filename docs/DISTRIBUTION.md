# `dev-prune` (`devp`) Multi-Ecosystem Distribution & Packaging Manual

Every way to install **`dev-prune`** (`devp`), what each channel actually ships, and the security guarantees behind them. For the maintainer's side — credentials, registry policies, what to do when a release fails — see [RELEASING.md](RELEASING.md).

---

<p align="center">
  <img src="../assets/github-readme-banner.png" alt="dev-prune — gigabytes back, nothing you can't rebuild" width="800" />
</p>

---

## 🔒 Security Audit & Privacy Guarantees

- **No analytics or diagnostics**: `dev-prune` never transmits workspace directory structures, repository names, user file paths, or usage data. Its single network request is a release check against GitHub's public releases page — see [PRIVACY.md](PRIVACY.md).
- **Subprocess Command Injection Prevention**: All lockfile verification commands (`npm`, `pnpm`, `yarn`, `bun`, `uv`, `cargo`, `go`) execute binary targets directly via `std::process::Command` without shell expansion.
- **Atomic State Storage**: `registry.json` is never written in place. Each update is written in full to a `.tmp` file and then renamed over the target, so an interrupted or failed write leaves the previous registry intact rather than a half-written one.
- **Sandboxed Scope**: File operations are strictly bounded to verified Git workspaces (`.git` presence) and to the directories an adapter names by hand — `node_modules`, `.venv`, `vendor`, `Pods` and the rest. The set is fixed in the source, one entry per package manager; nothing is deleted because it matched a pattern or a `.gitignore` rule.

---

## 🚀 Active & Live Distribution Channels

### 1. Universal One-Liner Shell Installer (macOS & Linux) — **LIVE**
```bash
curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
```
- Installs the pre-built binary to the platform config directory: `~/Library/Application Support/dev-prune/bin/` on macOS, `$XDG_CONFIG_HOME/dev-prune/bin/` (default `~/.config/…`) on Linux — the same directory dev-prune reads its registry from.
- Verifies the download against the release's published SHA-256 checksum and refuses to install if it does not match or is absent.
- Adds that directory to `PATH` in whichever of `.zshrc`, `.bashrc` and `config.fish` exist, and prints the `export` line to paste when none do. `devp` is a second copy of the binary in the same directory, not a shell alias, so it works in every shell — including ones whose startup file was never touched.
- Runs `dev-prune setup` as its last step, installing the integrations described in [Background Automation](BACKGROUND_AUTOMATION.md). `--no-auto-setup` installs the binary and nothing else, and `devp uninstall` reverses it either way.
- Registers **no** repositories. Which directories to track stays your decision: run `dev-prune init <dir>` yourself.
- Asks exactly one question, and only when there is somebody to answer it: if another manager's copy is on `PATH`, whether to collapse the two by running `devp install --channel installer` from that older binary. Anything but `y` prints the command instead. `DEV_PRUNE_NO_MIGRATE_PROMPT=1`, a set `CI`, or no terminal at all skips the question and prints the command. Nothing is deleted by the script either way.
- Writes `install.json` beside the binary as its last step — version, which script, when, and whether `devp` and the `PATH` entry came from it. `devp doctor` and `devp install` report it; nothing decides anything from it.
- `curl … | sh` runs in a child process, so it cannot change the PATH of the shell you typed it in. Open a new terminal, or run the `export` line the installer prints. (The PowerShell one-liner *can*, and does — see below.)
- Safe to re-run, and quiet about it. An install already at this version, with `devp` beside it and on `PATH`, is left exactly as it is and exits `0` without downloading anything; an older one is updated in place; a newer one is not downgraded unless you name the version with `--version`. A same-version install missing `devp` or its `PATH` entry is repaired. An install pinned with `devp config set version_lock true` is left alone whatever its version. `--force` downloads and writes it again regardless, including over a pin.
- Options: `--version <tag>`, `--bin-dir <dir>`, `--no-path`, `--no-auto-setup`, `--force`, `--help`. Piping into a shell needs `-s --` to reach them:
  ```bash
  curl -fsSL https://devprune.vkrishna04.me/install.sh | sh -s -- --no-auto-setup
  ```
  The environment variables `DEV_PRUNE_VERSION`, `DEV_PRUNE_BIN_DIR`, `DEV_PRUNE_NO_PATH=1`, `DEV_PRUNE_NO_AUTO_SETUP=1` and `DEV_PRUNE_FORCE=1` do the same and work with the plain one-liner. An option wins over its variable. `DEV_PRUNE_NO_MIGRATE_PROMPT=1` has no option of its own and suppresses the question above.

### 2. Windows One-Liner PowerShell Installer — **LIVE**
```powershell
iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex
```
- Downloads the pre-built Windows executable to `%APPDATA%\dev-prune\bin\dev-prune.exe` and verifies its published SHA-256 checksum before installing.
- Registers that directory in the User `PATH`, **and** in the current session's `$env:PATH`. `iwr … | iex` runs inside your own process, so `devp` works on the very next line you type — `iwr -useb … | iex; devp init ~/Code` behaves exactly as written.
- Installs `devp.exe` as a second copy of the binary rather than a `$PROFILE` function, so it works in cmd, PowerShell, Git Bash, an IDE terminal and a scheduled task alike, and cannot go stale against a profile nobody re-sources.
- Replaces a running executable safely: the old image is renamed aside before the new one is copied in, which is the only thing Windows permits while a prune pass happens to be mid-run.
- Runs `dev-prune setup` as its last step, installing the integrations described in [Background Automation](BACKGROUND_AUTOMATION.md). `-NoAutoSetup` installs the binary and nothing else, and `devp uninstall` reverses it either way.
- Registers **no** repositories. Run `dev-prune init <dir>` yourself.
- Asks exactly one question, and only when there is somebody to answer it: if another manager's copy is on `PATH`, whether to collapse the two by running `devp install --channel installer` from that older binary. Anything but `y` prints the command instead. `DEV_PRUNE_NO_MIGRATE_PROMPT=1`, a set `CI`, or a host with no desktop behind it skips the question and prints the command. Nothing is deleted by the script either way.
- Writes `install.json` beside the binary as its last step — version, which script, when, and whether `devp.exe` and the `PATH` entry came from it. `devp doctor` and `devp install` report it; nothing decides anything from it.
- Safe to re-run, and quiet about it. An install already at this version, with `devp.exe` beside it and on the User `PATH`, is left exactly as it is and returns without downloading anything; an older one is updated in place; a newer one is not downgraded unless you name the version with `-Version`. A same-version install missing `devp.exe` or its `PATH` entry is repaired. An install pinned with `devp config set version_lock true` is left alone whatever its version. `-Force` downloads and writes it again regardless, including over a pin.
- Parameters: `-Version <tag>`, `-BinDir <dir>`, `-NoPath`, `-NoAutoSetup`, `-Force`, `-Help`. `iwr … | iex` runs the script as a bare expression, which has nowhere to put arguments, so passing one means running it as a script block:
  ```powershell
  & ([scriptblock]::Create((iwr -useb https://devprune.vkrishna04.me/install.ps1))) -NoAutoSetup
  ```
  The environment variables `DEV_PRUNE_VERSION`, `DEV_PRUNE_BIN_DIR`, `DEV_PRUNE_NO_PATH=1`, `DEV_PRUNE_NO_AUTO_SETUP=1` and `DEV_PRUNE_FORCE=1` do the same and work with the plain one-liner. A parameter wins over its variable. `DEV_PRUNE_NO_MIGRATE_PROMPT=1` has no parameter of its own and suppresses the question above.
- From `cmd.exe`, which has no `Invoke-WebRequest`, the same script runs through PowerShell:
  ```bat
  powershell -NoProfile -ExecutionPolicy Bypass -Command "iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex"
  ```
  The install is identical. The one thing `cmd` loses is the current-session PATH update above — a parent shell cannot inherit the environment of the child it spawned — so `devp` resolves in the next Command Prompt rather than immediately. `-ExecutionPolicy Bypass` is defensive rather than required: the policy governs script *files*, and `iwr … | iex` never creates one.

### 3. Pre-Compiled GitHub Release Binaries
Seven single-binary archives are built automatically for every tagged release and attached to [GitHub Releases](https://github.com/Life-Experimentalist/dev-prune/releases), each with a `.sha256` sidecar in `sha256sum` format:

| Asset | Rust target |
|---|---|
| `dev-prune-v1.18.0-windows-x64.zip` | `x86_64-pc-windows-msvc` |
| `dev-prune-v1.18.0-windows-arm64.zip` | `aarch64-pc-windows-msvc` |
| `dev-prune-v1.18.0-windows-x86.zip` | `i686-pc-windows-msvc` |
| `dev-prune-v1.18.0-darwin-x64.tar.gz` | `x86_64-apple-darwin` |
| `dev-prune-v1.18.0-darwin-arm64.tar.gz` | `aarch64-apple-darwin` |
| `dev-prune-v1.18.0-linux-x64.tar.gz` | `x86_64-unknown-linux-musl` |
| `dev-prune-v1.18.0-linux-arm64.tar.gz` | `aarch64-unknown-linux-musl` |

The Linux binaries are statically linked against musl. There is no glibc version floor and no per-distribution build: the same `linux-x64` archive runs on Debian, Fedora, Arch, NixOS and Alpine. Pick by CPU architecture and nothing else.

**32-bit is published for Windows only.** `x64` is x86-64 (Intel/AMD, also called AMD64), `arm64` is AArch64, and `x86` is 32-bit x86 — the `i686-pc-windows-msvc` build, for machines with no 64-bit mode at all: locked-down corporate images, industrial control PCs, the last generation of Atom netbooks. There is no 32-bit Linux, no 32-bit macOS (Apple removed the ability to run one in Catalina) and no 32-bit ARM build anywhere.

A 32-bit *process* on 64-bit Windows still gets the x64 archive: `install.ps1` reads `PROCESSOR_ARCHITEW6432`, which is the machine's architecture rather than the shell's, so the `x86` asset goes only to hardware that can run nothing else. For anything else 32-bit, `cargo install dev-prune` on that toolchain works — nothing in the source is 64-bit-only.

The install scripts construct these filenames by hand and refuse to install without the matching `.sha256`, so the naming is a contract rather than a convention.

**Each Windows zip also ships a `.zip.contents.sha256`** — the digest of every file *inside* the archive, in the same `sha256sum` format. An archive's hash stops meaning anything the moment you unpack it, and the unpacked files are the only ones an antivirus ever looks at, so without this there was no published answer to "is the `devpw.exe` on my disk the one you built?". Run it in the folder you extracted to, from Git Bash — `sha256sum` ships with Git for
Windows, and is what reads this format:

```bash
sha256sum -c dev-prune-v1.18.0-windows-x64.zip.contents.sha256
```

`dev-prune.exe` and `devp.exe` are one file under two names and share a digest on purpose, so a scanner builds one reputation record instead of two — the matching digests are themselves the evidence the packaging did what it claims. `devpw.exe`, the console-free build the scheduled task runs, is a separate `[[bin]]` target and legitimately has a different one. [`devp trust`](CLI_REFERENCE.md#18-devp-trust---json---fix-ownership) prints the same digests off your own disk, which is the side of the comparison that matters.

Each archive is additionally signed with GitHub build provenance, which ties it to this repository, the release workflow and the commit it was built from — something a checksum cannot do, because whoever produces an archive also produces its checksum. Verify with no key and no account:

```bash
gh attestation verify dev-prune-v1.18.0-linux-x64.tar.gz --repo Life-Experimentalist/dev-prune
```

### 4. npm (`npm install -g` / `npx` / bun / pnpm / Yarn)
```bash
npm install -g dev-prune@latest   # installs, and updates an existing copy
npx dev-prune status              # run once, nothing installed
bun add -g dev-prune@latest       # same package, through bun
pnpm add -g dev-prune@latest
yarn global add dev-prune@latest  # Yarn 1.x
```
Every release publishes here. The channel was completed in 1.8.0, when the last three of
the eight package names were claimed; 1.6.0 and 1.7.0 published a Linux and macOS half.

- The tarball **contains the binary**. There is no `postinstall` download step, so the package installs correctly under `npm ci --ignore-scripts`, behind a corporate registry mirror, and with no network access to GitHub.
- Eight packages make that work: seven platform packages (`dev-prune-linux-x64`, `dev-prune-darwin-arm64`, `dev-prune-windows-x64`, …), each carrying one executable and declaring `os`/`cpu`, plus the `dev-prune` dispatcher that lists all seven as `optionalDependencies`. npm resolves exactly the one that matches the machine and skips the rest.
- The three Windows packages are named `dev-prune-windows-x64`, `dev-prune-windows-arm64` and `dev-prune-windows-x86` — after the release assets, not after npm's `win32`/`ia32` platform strings, which the registry refused as new names. Each one still declares `win32` and `ia32` in `os` and `cpu`, which is what npm actually resolves against, so only the name differs.
- Both `dev-prune` and `devp` are registered as `bin` entries.
- Every tarball carries [npm provenance](https://docs.npmjs.com/generating-provenance-statements) — a signed attestation tying it to the workflow run, commit and tag that produced it. Publishing uses npm Trusted Publishing: the workflow's OIDC token is the whole credential and no npm token exists anywhere.
- Upgrading from a Windows machine that still holds 1.7.0 needs `npm install -g dev-prune@latest`. A published manifest cannot be edited, so `dev-prune@1.7.0` names the old, never-created Windows packages permanently and no repair reaches it.
- **bun, pnpm and Yarn install the same package, and each is its own channel.** The dispatcher-plus-platform-package layout means every one of the four clients ends up with the executable inside a `node_modules` tree, so the location alone cannot tell them apart; dev-prune checks each client's own global directory (`~/.bun`, pnpm's store, `~/.config/yarn/global`) before falling back to npm. That matters because the managers do not share records: `npm install -g dev-prune@latest` against a copy bun installed adds a *second* copy under npm's prefix and leaves bun's, still on `PATH`, at the old version. `devp update`, `devp doctor` and `devp uninstall` all name the client that actually owns the copy. Deno is not a channel: `deno install -g npm:dev-prune` writes a shim that re-enters Deno, so the running executable is Deno itself and there is nothing for the classifier to recognise.

### 5. PyPI (`uv tool install` / `uvx` / `pipx` / `pip`)
```bash
uv tool install dev-prune@latest   # installs, and updates an existing copy
uvx dev-prune status               # run once, nothing left behind
pipx run dev-prune status
pip install --upgrade dev-prune
```
- Six platform wheels, each a zip holding the prebuilt executable under `dev_prune-<version>.data/scripts/`. No Python runs, no compiler is invoked, and no build backend is involved — installers unpack the binaries straight into the environment's `bin`/`Scripts` directory.
- The Linux wheels carry both `manylinux` and `musllinux` tags from the same static binary, so Debian and Alpine users are both served.
- Uploaded through PyPI [Trusted Publishing](https://docs.pypi.org/trusted-publishers/): no API token exists anywhere, only a short-lived OIDC credential minted per release.
- A `pip install` inside a virtualenv is fine: the first run copies the binary to the managed location (`%APPDATA%\dev-prune\bin` / `~/.config/dev-prune/bin`) and puts it on `PATH`, so `devp` survives the venv being deactivated or deleted. Details in [INSTALLATION_ISSUES.md §9](troubleshooting/INSTALLATION_ISSUES.md#9-pip-install-in-a-virtual-environment--what-happens-when-the-venv-goes-away).

### 6. Cargo / crates.io (`cargo binstall` or `cargo install`)
```bash
cargo binstall dev-prune   # downloads the release archive
cargo install dev-prune    # compiles from source
```
crates.io hosts source, not binaries — there is no executable on the registry for `cargo install` to fetch, so it always builds, and it is the only channel that does. Requires Rust 1.88+ (edition 2024) and the release profile below.

**Does `cargo install` produce a faster binary tuned to your machine?** No, and it does not need to. It compiles for your host target triple with the same `[profile.release]` settings the prebuilt archives use (`lto`, `codegen-units = 1`, `opt-level = 3`), but with a *generic* CPU baseline — it does **not** pass `-C target-cpu=native`, so it does not emit instructions specific to your exact processor. The result is byte-for-byte equivalent in optimization level to the prebuilt binaries and performs the same. dev-prune spends its time waiting on the filesystem and on package-manager subprocesses, not in hot numeric loops, so a `target-cpu=native` build would not be measurably faster anyway. The practical differences from `cargo install` are only that it needs a Rust toolchain and takes minutes instead of seconds. **The prebuilt channels above already cover every supported platform; there is nothing `cargo install` reaches that they miss.**

Every channel — including `cargo install` and a bare unzipped archive — is complete on its own. On Windows the windowless scheduler binary (`devpw.exe`, see [Background Automation](BACKGROUND_AUTOMATION.md)) is generated locally beside the installed binary on first setup, so no channel has to package a second executable for it.

[`cargo binstall`](https://github.com/cargo-bins/cargo-binstall) closes the from-source gap. The `[package.metadata.binstall]` table in `Cargo.toml` names one GitHub release asset per target — the same six archives listed above — so binstall resolves the version on crates.io, downloads the matching archive, and unpacks the executable without a toolchain. The table restates the asset names from `release.yml`; if an asset is ever renamed and the table is not, `cargo binstall` silently falls back to compiling, which is the only symptom.

### 7. Homebrew (macOS & Linux)

```bash
brew tap Life-Experimentalist/tap
brew install dev-prune
```

Or without tapping anything, straight from the formula's URL:

```bash
brew install https://raw.githubusercontent.com/Life-Experimentalist/dev-prune/main/packaging/homebrew/dev-prune.rb
```

Installing a formula by URL needs no tap and no review — Homebrew fetches the file, checks the archive against the `sha256` in it, and installs. The formula covers macOS x64/arm64 and Linux x64/arm64, installs `devp` as a symlink to the same binary, and generates the shell completions from the binary itself.

The two spellings install the same file. The difference is afterwards: a formula installed by URL is not attached to any tap, so `brew upgrade` has nowhere to look and never offers you a new version. The tap — [`Life-Experimentalist/homebrew-tap`](https://github.com/Life-Experimentalist/homebrew-tap) — exists for that, and it holds nothing but this one formula, pulled from `packaging/homebrew/` here on a daily schedule.

Plain `brew install dev-prune`, with no tap prefix, means homebrew-core, which has a notability bar and stays on [ROADMAP.md](ROADMAP.md).

### 8. Scoop (Windows)

```powershell
scoop bucket add life-experimentalist https://github.com/Life-Experimentalist/scoop-bucket
scoop install dev-prune
```

Or without adding the bucket, straight from the manifest's URL:

```powershell
scoop install https://raw.githubusercontent.com/Life-Experimentalist/dev-prune/main/packaging/scoop/dev-prune.json
```

Scoop installs a manifest by URL for the same reason Homebrew does: the manifest carries the download URL and its hash, so there is nothing left to trust. It covers x64, arm64 and 32-bit Windows, and registers both `dev-prune` and `devp` as commands.

As with Homebrew, the URL form installs but never upgrades — `scoop update dev-prune` needs the manifest to belong to a bucket. [`Life-Experimentalist/scoop-bucket`](https://github.com/Life-Experimentalist/scoop-bucket) is that bucket, and holds nothing but this one manifest, pulled from `packaging/scoop/` here on a daily schedule.

The manifest also carries `checkver` and `autoupdate`, so any other bucket that adopts it can bump itself from the release tag and the published `.sha256` sidecars without waiting for anyone.

### 9. WinGet — not yet available

```powershell
winget install VKrishna04.dev-prune
```

`winget-pkgs` has no popularity requirement, but every version is a pull request reviewed by a person, so it is not something a release job can do unattended. What the release *does* do is render the three manifests it needs — version, installer and locale — against the assets it just published, into [`packaging/winget/`](../packaging/winget/). Submitting them is a maintainer step documented in [RELEASING.md](RELEASING.md).

dev-prune is not currently in `winget-pkgs`, so **the command above does not resolve**; until a submission is merged, use Scoop or the install script.

One detail worth knowing if you install this way. A portable WinGet install cannot publish two command names from one file — a repeated `RelativeFilePath` is a manifest error — so the Windows archive carries `dev-prune.exe` and `devp.exe` as two real files, and WinGet puts both on your PATH at install time. Nothing is created on first run, which matters here: WinGet versions its package directory and replaces it wholesale on upgrade, so anything written beside the binary there would be orphaned by the next upgrade while still sitting on your PATH. The managed pair described in [Background Automation](BACKGROUND_AUTOMATION.md) lives in the config directory instead, where an upgrade cannot reach it.

---

## 🔀 Changing channels

One channel owns one binary. Each of the channels above keeps its own copy in its own
directory, and each upgrades only what it installed — `cargo install --force` will not
touch a WinGet package, and `devp update` upgrades the copy that is *running*, through
whichever manager put it there. Two managers writing the same PATH entry would fight
over it forever, so none of them try.

That leaves the question of how to move.

### Running the one-liner over an existing install

It is safe, and it is the one case worth describing precisely because it looks like it
did more than it did.

The install script writes its own copy into the managed `<config>/bin` directory and
leaves every other copy exactly where it is. It does not run `cargo uninstall`, `npm
uninstall -g` or `winget uninstall` on your behalf: deleting another package manager's
file behind its back leaves that manager's records claiming a version that is no longer
there, which is how an installation becomes unrepairable rather than merely wrong.

What it does do is put its directory **first** on your PATH — prepended in the shell rc
file on macOS and Linux, prepended in the User PATH on Windows — so `devp` afterwards is
the copy it just installed, in this terminal and in every later one. Then it names the
other copy and offers to collapse the two:

```text
[!] Another dev-prune is on your PATH as well:
        /home/you/.cargo/bin/dev-prune
    A different package manager owns that copy, so this script left it alone.
    This directory comes first on PATH, so 'devp' is the copy in /home/you/.config/dev-prune/bin.
    Moving it over means installing here and uninstalling there, through the
    manager that put it there. 'devp install --channel installer' does both.
    Do that now? [y/N]:
```

Answering `y` runs `devp install --channel installer --yes` from the *old* binary, which
is the copy that knows which manager owns it. Anything else prints the command and moves
on, and where there is nobody to ask — `DEV_PRUNE_NO_MIGRATE_PROMPT=1`, a set `CI`, no
terminal attached — the question is skipped and the command printed. The script itself
deletes nothing in any of those cases.

So the answer to “does the one-liner work if I installed some other way first?” is yes,
on every channel, with no uninstall step first. Say yes to the question and the two
collapse there and then; say nothing and you are left with a second, older copy that
nothing upgrades. That is untidy rather than broken — but it is the copy that starts
running the day the new one is removed, so it is worth finishing the job.

### Moving properly: `devp install --channel`

```bash
devp install                              # which manager owns the running copy?
devp install --channel winget --dry-run   # print the numbered plan, run none of it
devp install --channel winget             # install there, then uninstall here
```

It does the two halves in the order that leaves a working `devp` at every point in
between: install through the new manager **first**, then remove the old copy through the
manager that owns it. An install that fails leaves the old copy exactly where it was.
Removing the old one through its own manager rather than deleting the file is the whole
point — a manager whose records still say dev-prune is present will put it back.

Nothing has to be migrated. Settings, the repository registry and the undo history live
in the config directory, which no channel owns and none of them touch.

Names `--channel` accepts: `installer`, `cargo`, `npm`, `bun`, `pnpm`, `yarn`, `uv`,
`pipx`, `winget`, `scoop`, `homebrew`. The three npm-compatible clients install the same
package but are each their own channel, because each keeps its own record of it. Full reference: [CLI_REFERENCE.md](CLI_REFERENCE.md#19-devp-install---channel-name---dry-run).

### Finding copies you have forgotten

```bash
devp doctor
```

`doctor` reports the channel that owns the running binary, and searches both your PATH
and every channel's fixed install directory — including directories that are *not* on
PATH, because a copy nobody can see is also a copy nobody upgrades. Any copy running a
different version is listed by path and version. It never deletes one: which copy you
want is a question only you can answer.

---

## 📦 Release profile

All published binaries are built with `lto = true`, `codegen-units = 1`, `strip = true` and `opt-level = 3`.

---

## 🛠️ Maintainer release workflow

A release is one `git push` of one tag. Nothing is built, archived, uploaded or published by hand:

```bash
git tag -a v1.3.0 -m "v1.3.0" && git push origin v1.3.0
```

`.github/workflows/release.yml` then builds all six targets, verifies the tag matches `Cargo.toml` and that `CHANGELOG.md` documents it, publishes a GitHub Release whose body is that changelog section, and pushes to PyPI and crates.io (the npm job is gated off — see section 4).

**[docs/RELEASING.md](RELEASING.md)** has the whole process: the credentials each registry needs, which registries review submissions (npm, PyPI and crates.io do not), what to do when a release goes wrong, and the gated channels — Homebrew, WinGet, Scoop — that are not automated because a human sits on the other side.
