# 🚢 Releasing `dev-prune`

Everything needed to cut a release, and everything that has to exist on the outside
world for one to succeed. Written for a maintainer, not a user.

The short version: **a release is one `git push` of one tag.** Nothing is built,
uploaded, or published by hand. If a step below sounds like manual work, it is either
one-time setup or a pre-flight check.

---

## 📋 Contents

- [Launch day: the first release](#-launch-day-the-first-release)
- [The one-time setup](#-the-one-time-setup)
- [Cutting a release](#-cutting-a-release)
- [What the workflow does](#-what-the-workflow-does)
- [The changelog contract](#-the-changelog-contract)
- [Registry reference: who reviews what](#-registry-reference-who-reviews-what)
- [Why a Rust binary is welcome on npm and PyPI](#-why-a-rust-binary-is-welcome-on-npm-and-pypi)
- [Channels that still need a human](#-channels-that-still-need-a-human)
- [When a release goes wrong](#-when-a-release-goes-wrong)

---

## 🚀 Launch day: the first release

Every release after the first is [one tag push](#-cutting-a-release). The first one also
has to bring the outside world into existence. In order, because several steps depend on
the one above:

1. **Create the repository** as `Life-Experimentalist/dev-prune`, **public**. Public is
   not cosmetic: `npm publish --provenance` fails on a private repository, and the
   release check reads the public releases endpoint unauthenticated.

   ```bash
   gh repo create Life-Experimentalist/dev-prune --public --description "Reclaims disk space from idle Git repositories by deleting only what a lockfile can rebuild." --homepage "https://devprune.vkrishna04.me"
   ```

   Then set the topics, which are what GitHub search and the topic pages index on:

   ```bash
   gh repo edit Life-Experimentalist/dev-prune --add-topic rust,cli,developer-tools,disk-space,node-modules,monorepo,cleanup,devtools
   ```

   The social preview has no API and has to be uploaded by hand: Settings → General →
   Social preview → Upload `assets/readme-banner.png`. It is already exactly the
   1280×640 GitHub asks for, so nothing is rescaled. It is the image every link to the
   repository renders as on Twitter/X, Slack, Discord and Hacker News, and without it
   they render as a grey octocat.

2. **Push `main`.** CI runs immediately — ten jobs, including `cargo package`, both
   packaging scripts against fabricated assets, the ARM64 Windows cross-check, and the
   changelog gate. Nothing below is worth doing until it is green.

   ```bash
   git remote add origin https://github.com/Life-Experimentalist/dev-prune.git && git push -u origin main
   ```
3. **Turn on Pages**: Settings → Pages → Source → **GitHub Actions**. Then point DNS at
   it — a `CNAME` record for `devprune` at `Life-Experimentalist.github.io` — and tick
   *Enforce HTTPS* once the certificate is issued. `site/public/CNAME` already carries
   the hostname, so nothing in the repository needs editing.

   Pages is off by default and the API says so plainly, which is a faster answer than
   waiting on DNS:

   ```powershell
   gh api repos/Life-Experimentalist/dev-prune/pages
   ```

   `HTTP 404` means Pages has never been enabled. Once it is, that returns the live URL
   and the certificate state.
4. **Prove the install scripts are actually served.** The one-liner in the README, the
   site and every doc points at this URL, and it is the single most-run command in the
   project:

   ```bash
   curl -fsSL https://devprune.vkrishna04.me/install.sh | head -5
   ```

5. **Add the credentials** — [npm](#npm), [PyPI](#pypi-trusted-publishing--no-token-anywhere)
   and [crates.io](#cratesio). Each channel needs a secret *and* its `*_PUBLISH`
   variable; a channel with neither is skipped, so a partial set is a valid way to
   start and the rest begin working on the next release with no code change.

   Confirm it rather than assuming it — this is the step whose omission is invisible
   until after the tag exists:

   ```powershell
   gh secret list; gh variable list
   ```

   You want `NPM_TOKEN` and `CARGO_REGISTRY_TOKEN` in the first list, and
   `NPM_PUBLISH`, `CRATES_PUBLISH` and `PYPI_PUBLISH` in the second. Empty output means
   nothing is configured, and the release will reach the GitHub release page only.
6. **Check the changelog date.** `CHANGELOG.md` dates the section for the version being
   released, and the date should be the day it actually ships. Fix it in place if the
   calendar has moved on.
7. **Read the release body before it becomes one.** This exact text is what appears on
   the GitHub release page:

   ```bash
   sh scripts/changelog-section.sh 1.2.0
   ```

   On Windows, `sh` is not on `PATH` but Git ships one, and running the real extractor
   beats reimplementing it — a second copy is a second thing to keep in step:

   ```powershell
   & "$env:ProgramFiles\Git\bin\sh.exe" scripts/changelog-section.sh 1.2.0
   ```

8. **Tag and push.**

   ```bash
   git tag -a v1.2.0 -m "v1.2.0" && git push origin v1.2.0
   ```

   PowerShell has no `&&`; use `;` and check in between, or just run the two separately.

9. **Verify each channel** once the workflow finishes. These are the commands users will
   actually run, and running them is the only proof the packages resolve:

   ```bash
   uvx dev-prune@1.2.0 -V
   cargo install dev-prune --version 1.2.0
   cargo binstall dev-prune@1.2.0
   curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
   ```

   `npx dev-prune@<version> -V` joins this list only once npm publishing is switched on
   — `NPM_PUBLISH` is currently `false` and nothing has ever shipped to npm.

   `cargo binstall` is the one worth watching: it reads
   `[package.metadata.binstall]` from the published crate and downloads a release asset
   by name. If an asset was renamed without updating that table, binstall does not fail
   — it quietly falls back to compiling from source, which looks like success and takes
   two minutes instead of two seconds. Run it with `--no-cleanup` and read the log.

   The last one is the shell installer. Its Windows counterpart is a different script and
   a different one-liner — `curl` in PowerShell is an alias for `Invoke-WebRequest` and
   will not behave like the binary, so verify the PowerShell path on PowerShell:

   ```powershell
   iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex
   ```

The `dev-prune` name is now held on crates.io and PyPI, where 1.0.0 through 1.2.0 are
published. On npm it is still unclaimed — see [Name availability](#name-availability) —
and "unclaimed" has a shelf life, so switching `NPM_PUBLISH` on is also what secures the
name there.

---

## 🔑 The one-time setup

Five channels publish automatically, and **each one is off until you switch it on**. A
channel that is off reports `skipped`, not `success` — so the run page tells you which
registries actually received the release, rather than looking identical either way.

Every channel needs **two** things: the credential, and a repository *variable* saying
the channel is configured. The variable exists because GitHub does not make the `secrets`
context available to a job-level `if:`, so a missing secret can only be detected inside
the job — by which point the job already exists and will report success when it returns
early. The variable is checked before the job starts.

Once the variable is `true`, a missing or mistyped secret **fails the release loudly**
instead of quietly skipping it.

| Channel | Credential | Variable to set | Off ⇒ |
|---|---|---|---|
| GitHub Release | `GITHUB_TOKEN` | — always runs | n/a |
| npm | `NPM_TOKEN` secret | `NPM_PUBLISH` = `true` | Job reports `skipped` |
| PyPI | *no secret* — Trusted Publishing | `PYPI_PUBLISH` = `true` | Job reports `skipped` |
| crates.io | `CARGO_REGISTRY_TOKEN` secret | `CRATES_PUBLISH` = `true` | Job reports `skipped` |
| GitHub Pages (site) | Automatic | — Settings → Pages → "GitHub Actions" | Site does not deploy |

Secrets and variables live in the same place, on two different tabs: **Settings → Secrets
and variables → Actions**. Putting a variable's value in the Secrets tab is the easiest
way to get a release that publishes nothing.

Whatever the outcome, the **What actually shipped** job writes a per-channel table to the
run summary, and warns if the release reached the GitHub release page and no registry.

### npm

1. Create an account on [npmjs.com](https://www.npmjs.com/) and enable 2FA.
2. Create a **Granular Access Token** (Access Tokens → Generate New Token → Granular).
   - Permissions: **Read and write** on packages.
   - Scope it to the eight package names below, or leave it account-wide before the
     first publish — granular tokens cannot name a package that does not exist yet.
   - **Tick "Bypass two-factor authentication".** Without it the registry answers
     `403 ... Two-factor authentication or granular access token with bypass 2fa enabled
     is required to publish packages`, because the account requires 2FA for writes and a
     workflow has no way to answer an OTP prompt. The box can only be ticked when the
     token is created; an existing token cannot be edited to add it.
   - Set an expiry you will actually remember. 90 days is the default; a year is fine
     for a personal project.
3. Save it as the repository secret **`NPM_TOKEN`**.
4. Set the repository **variable** **`NPM_PUBLISH`** to `true`. Without it the publish
   job does not run at all, and the release reports `skipped` for npm.

The eight packages the release publishes — the same eight `npm/package.json` names,
which is where this list has to keep agreeing with reality:

```
dev-prune                  ← the dispatcher everyone installs
dev-prune-linux-x64
dev-prune-linux-arm64
dev-prune-darwin-x64
dev-prune-darwin-arm64
dev-prune-win32-x64
dev-prune-win32-arm64
dev-prune-win32-ia32
```

Nothing needs to be reserved in advance — the first publish creates all eight. The seven
platform packages are published *before* the dispatcher, because npm resolves the
dispatcher's `optionalDependencies` as soon as it exists.

**A `404 Not Found` on the `PUT` is this token being wrong, not the package being
missing.** A granular token that names packages can only see the ones it names, so a name
that does not exist yet is not "forbidden" to it — it is invisible, and npm says so with
a 404. v1.6.0 hit exactly this. The fix is a classic automation token, or a granular one
with read/write on *all* packages, held only until the eight names exist.

Publishes use `--provenance --access public`, which requires the repository to be
**public** and the workflow to have `id-token: write`. It attaches a signed attestation
tying each tarball to this workflow, this commit and this tag; npm shows it as a green
"Provenance" badge on the package page. A private repo makes the publish fail — drop the
`--provenance` flag in that case.

`--access public` is not optional even though these are unscoped names. npm refuses to
generate provenance for a package it cannot confirm is public, and a package with no
published versions has no access setting to read, so the first publish of each name
fails with `EUSAGE ... you must set access to public` without it.

#### Bootstrapping the eight names without a token at all

The token above is one way past the chicken-and-egg. The better way is to publish the
first version from a workstation, because an interactive `npm login` session answers the
2FA challenge itself:

```sh
gh release download v1.2.0 --dir /tmp/rel
cd /tmp/rel && sha256sum -c ./*.sha256          # publish the CI binaries, not local ones
sh scripts/npm-prepare.sh 1.2.0 /tmp/rel /tmp/npm-dist
sh scripts/npm-publish.sh 1.2.0 /tmp/npm-dist latest --local
```

`--local` drops `--provenance`, which is not a preference: the attestation is signed
against a CI OIDC identity and a workstation has none. **The version published this way
carries no provenance badge.** Every later version, published by CI, does.

Re-run the command as often as you like — the `npm view` check skips whatever already
made it to the registry, which matters here because a 2FA code expires partway through
seven publishes more often than not.

Always prepare from the downloaded release assets rather than a local `cargo build`.
The npm packages must contain the same executables the tarballs, wheels and installers
ship, and the `.sha256` files next to the assets are what proves it.

#### Move to Trusted Publishing once the names exist

The `NPM_TOKEN` above is a bridge, not the destination. **npm removes direct publish
access from bypass-2FA tokens in January 2027**, and a long-lived token that can publish
eight packages is the exact thing the 2025 supply-chain attacks abused.

The reason it is still here is bootstrapping: npm can only trust a publisher that is
configured *on a package*, and a package that has never been published does not exist to
configure. PyPI solves this with pending publishers; npm has no equivalent. So the first
publish of a new name must use a token, and every later one need not.

Once a first version is on the registry, for **each of the eight packages**: npmjs.com → the
package → Settings → Trusted Publisher → GitHub Actions → repository
`Life-Experimentalist/dev-prune`, workflow `release.yml`. Then delete the `NPM_TOKEN`
secret and drop the `NODE_AUTH_TOKEN` guard from `publish-npm`; OIDC replaces it, and
provenance is generated automatically rather than by the flag.

### PyPI (Trusted Publishing — no token anywhere)

1. Create an account on [pypi.org](https://pypi.org/) and enable 2FA.
2. Go to **Your projects → Publishing → Add a new pending publisher** and fill in:

   | Field | Value |
   |---|---|
   | PyPI Project Name | `dev-prune` |
   | Owner | `Life-Experimentalist` |
   | Repository name | `dev-prune` |
   | Workflow name | `release.yml` |
   | Environment name | `pypi` |

   These four values are matched exactly against the OIDC token GitHub mints. The
   workflow filename and the `environment: pypi` block in `release.yml` are load-bearing
   — renaming either breaks publishing until PyPI is updated to match.
3. In the repository, create the environment: **Settings → Environments → New
   environment → `pypi`**. No secrets go in it. Add a required reviewer if you want a
   human approval gate before each PyPI upload.
4. Set the repository **variable** (not secret) **`PYPI_PUBLISH`** to `true`:
   Settings → Secrets and variables → Actions → Variables → New repository variable.

   The gate is a variable because Trusted Publishing has no secret to test for — without
   it, the job would hard-fail on every release until PyPI was configured.

### crates.io

1. Sign in at [crates.io](https://crates.io/) with GitHub.
2. Account Settings → API Tokens → New Token. Scopes: `publish-new` and `publish-update`.
3. Save it as the repository secret **`CARGO_REGISTRY_TOKEN`**.
4. Set the repository **variable** **`CRATES_PUBLISH`** to `true`.

### Name availability

As of 2026-08-20, `dev-prune` is published (1.0.0 through 1.2.0) on PyPI and crates.io,
which is what holds a name on those registries. Nothing has been published to npm —
`NPM_PUBLISH` is `false` — so every name there is still unclaimed:

| Name | npm | PyPI | crates.io |
|---|---|---|---|
| `dev-prune` | free | **held (published)** | **held (published)** |
| `devp` | free | free | free |
| `devprune` | free | free | free |

`Cargo.toml`, `npm/package.json` and `scripts/build_wheels.py` all publish as
**`dev-prune`**. Registering `devp` and `devprune` as placeholders is optional; npm and
PyPI both discourage name-squatting, and neither has a redirect mechanism, so a
placeholder is only worth it if you intend to publish something real under it.

---

## 🏷️ Cutting a release

```bash
cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all
```

CI runs all of this on every push, so the only reason to run it locally is to avoid
tagging something that will fail.

Then:

1. **Bump the version:** `sh scripts/bump-version.sh 1.6.0`. It writes `Cargo.toml` and
   every file that restates the number by hand — the install scripts' offline fallback,
   the site's banner and `llms.txt`, the JSON-LD, `npm/package.json`, the `--json`
   samples in the CLI reference and the docs that quote a whole asset filename — then
   refreshes `Cargo.lock` (the release builds with `--locked` and fails on a stale one)
   and runs `sh scripts/check-version.sh` to prove it got all of them. That check also
   runs on every push and once more inside the release before anything is built, so a
   file the bump script does not yet know about stops the tag rather than shipping.
2. **Write the changelog entry.** See [the changelog contract](#-the-changelog-contract).
   CI fails if `CHANGELOG.md` has no section matching the version in `Cargo.toml`, so
   this is enforced, not merely encouraged.
3. **Commit and push to `main`.** Wait for CI to go green.
4. **Tag and push the tag:**

   ```bash
   git tag -a v1.2.0 -m "v1.2.0"
   git push origin v1.2.0
   ```

That is the release. A tag matching `v*` triggers everything below.

To re-run a release after fixing a failed job, use **Actions → Release → Run workflow**
and enter the tag. The npm job skips packages already on the registry, so a re-run
finishes what the first attempt started rather than dying on a conflict.

---

## ⚙️ What the workflow does

```mermaid
flowchart TD
    tag["git push origin v1.2.0"] --> build["build (6 matrix jobs)"]

    subgraph checks["Pre-flight, before any compilation"]
        v1["Cargo.toml version == tag"]
        v2["CHANGELOG.md has a section for it"]
    end

    build --> checks
    checks --> compile["cargo build --release --locked"]
    compile --> static["Linux binaries proven statically linked"]
    static --> pack["Archive + .sha256 sidecar per target"]

    pack --> publish["publish: GitHub Release"]
    publish --> npm["publish-npm: 7 packages"]
    publish --> pypi["publish-pypi: 6 wheels"]
    publish --> crate["publish-crate: cargo publish"]
```

**`build`** — seven targets, `fail-fast: false` so one broken platform does not hide the
others:

| Target | Asset |
|---|---|
| `x86_64-unknown-linux-musl` | `dev-prune-v<ver>-linux-x64.tar.gz` |
| `aarch64-unknown-linux-musl` | `dev-prune-v<ver>-linux-arm64.tar.gz` |
| `x86_64-apple-darwin` | `dev-prune-v<ver>-darwin-x64.tar.gz` |
| `aarch64-apple-darwin` | `dev-prune-v<ver>-darwin-arm64.tar.gz` |
| `x86_64-pc-windows-msvc` | `dev-prune-v<ver>-windows-x64.zip` |
| `aarch64-pc-windows-msvc` | `dev-prune-v<ver>-windows-arm64.zip` |
| `i686-pc-windows-msvc` | `dev-prune-v<ver>-windows-x86.zip` |

Each target also publishes the **same binary again, uncompressed**, under the archive's
name without the extension — `dev-prune-v<ver>-linux-x64`, `dev-prune-v<ver>-windows-x64.exe`
and so on. That is what `devp update --install` downloads: self-update needs one download
and one hash check, and unpacking a tarball inside the binary would mean compiling gzip
and tar into it purely to undo what the packaging step just did. The archives stay because
they are what a human downloads from the release page and what the install scripts fetch.
These names are a contract with `constants::release_asset_name`, which builds them by
hand — a mismatch is not a build failure, it is a self-update that 404s for every user on
release day, so the two are commented as referring to each other and covered by a unit
test.

Every asset gets a `.sha256` sidecar in `sha256sum` format. **The install scripts refuse
to install without it**, so the sidecars are a contract, not a courtesy — as are the
asset names themselves, which `scripts/install.sh` and `scripts/install.ps1` construct
by hand.

The archives are also signed with [GitHub build provenance][attest]: the `publish` job
runs `actions/attest-build-provenance`, which exchanges the job's OIDC token for a
Sigstore signature binding each file to this repository, this workflow file and the
commit it was built from. That is the part a checksum cannot do — a sidecar is produced
by whoever produced the archive, so a substituted pair verifies perfectly. Anyone can
check it with no secret and no key:

```bash
gh attestation verify dev-prune-v<ver>-linux-x64.tar.gz --repo Life-Experimentalist/dev-prune
```

Only the archives and the `.vsix` are attested. A sidecar is a checksum of an attested
file, so signing it adds nothing.

[attest]: https://docs.github.com/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds

Linux is musl and statically linked on purpose. A glibc build carries a floor: it
refuses to start on any distribution older than the runner that produced it, and does
not run on Alpine at all. A static binary has neither problem, so one asset per
architecture covers every distribution. The workflow proves the result is actually
static with `file` before packaging it, because a silently-dynamic binary would work on
the runner and fail only for users.

**`publish`** — attaches every asset to a GitHub Release whose body is the changelog
section for that version, with the auto-generated commit list appended below it.

**`publish-npm`** — `scripts/npm-prepare.sh` unpacks the assets into the eight packages
and publishes them, platform packages first. Prereleases (any tag containing `-`) go out
under the `next` dist-tag so they never become what `npm install dev-prune` resolves to.

**`publish-pypi`** — `scripts/build_wheels.py` turns the same assets into seven wheels
and uploads them via Trusted Publishing.

**`publish-crate`** — `cargo publish --locked`. Skipped for prereleases, because a
crates.io version can never be deleted, only yanked.

Both packaging scripts are exercised by the `packaging` job in CI on every push, against
fabricated assets. Their first real run is therefore not their first run.

The same reasoning covers `aarch64-pc-windows-msvc` and `i686-pc-windows-msvc`, the two
release targets that are not native builds on their own runner. CI's `cross` job
type-checks both on every push, so each Windows toolchain and its dependencies' native
code are known to work before a tag exists rather than after.

`i686-pc-windows-msvc` is the only 32-bit target published, and only for Windows. It
exists for machines with no 64-bit mode — locked-down corporate images, industrial
control PCs — and it is a plain rustup target on the same x64 runner, so it costs one
extra matrix job and nothing else. There is deliberately no 32-bit Linux build:
`i686-unknown-linux-musl` needs a cross musl toolchain to serve a desktop population
that has effectively gone.

---

## 📝 The changelog contract

`CHANGELOG.md` is the single source for release notes.
`scripts/changelog-section.sh <version>` extracts one version's section, and it is
called from three places: CI (on every push), the release build (before compiling), and
the release publish (as the release body). A release note and a changelog entry that can
drift apart eventually do — this makes them the same text.

The format is [Keep a Changelog](https://keepachangelog.com/) with
[SemVer](https://semver.org/):

```markdown
## [1.1.0] - 2026-09-01

### Added

- `devp run --except <name>` prunes everything **but** the repositories you name. …

### Fixed

- `devp unlink --missing` now also clears the undo list. …
```

Rules the automation depends on:

- The heading is exactly `## [<version>] - <YYYY-MM-DD>`. The extractor matches
  `## [<version>]` as a literal string prefix, so `1.0.0` will not accidentally match
  `1.0.0-rc1`.
- A section ends at the next `## ` heading. Anything between belongs to that version.
- The version must equal the one in `Cargo.toml` by the time you tag.
- Subsections use `### Added` / `### Changed` / `### Fixed` / `### Removed`.

House style for the entries themselves is in [`../CLAUDE.md`](../CLAUDE.md).

Verify a section renders the way you expect before tagging:

```bash
sh scripts/changelog-section.sh 1.1.0
```

---

## 🏛️ Registry reference: who reviews what

A common assumption is that publishing to a package registry involves review. For the
three registries used here, it does not.

| Registry | Human review? | What actually happens |
|---|---|---|
| **npm** | **None.** | `npm publish` makes the version live in seconds. Automated malware scanning runs after the fact and can result in takedown, not a hold. |
| **PyPI** | **None.** | Same: live immediately. Automated checks reject malformed metadata and oversized files; no person looks at the code. |
| **crates.io** | **None.** | Same. Publishing is permanent — a version can be *yanked* (hidden from resolution) but never deleted, and the name is never released. |
| **Homebrew core** | **Yes.** | A PR to `homebrew-core` reviewed by maintainers, with notability requirements (roughly 30+ forks / 30+ watchers / 75+ stars, or a clear equivalent). A **personal tap** has no review at all. |
| **WinGet** | **Yes.** | A PR to `microsoft/winget-pkgs`. Largely automated validation plus a maintainer sign-off; installer URL and hash must match. |
| **Scoop** | Depends. | The `extras` bucket is a reviewed PR; your own bucket is not. |
| **Chocolatey** | **Yes.** | Moderation queue, both automated and human. Historically the slowest of the lot. |
| **AUR** | **None.** | Anyone with an account can upload a PKGBUILD. |

So: npm, PyPI and crates.io are effectively free-for-all — the only gate is owning the
name. `dev-prune` is already held on PyPI and crates.io by the published releases; on
npm it is [still free](#name-availability) until `NPM_PUBLISH` is switched on.

The flip side of "no review" is that **nothing is reversible**. crates.io versions are
permanent; npm allows unpublishing only within 72 hours and only if nothing depends on
the package; PyPI lets you delete a release but never reuse that version number. This is
why every publish job is gated behind a successful build of all seven targets, and why
the version/changelog checks run *before* compilation rather than after.

---

## 🦀 Why a Rust binary is welcome on npm and PyPI

Neither registry requires the package to contain JavaScript or Python. Both are, in
practice, general-purpose binary distribution networks with excellent CDNs, and shipping
a compiled CLI through them is a mainstream pattern rather than an abuse of one.

**On npm**, the mechanism is `optionalDependencies` plus the `os` and `cpu` manifest
fields. A thin dispatcher package lists one package per platform; npm evaluates `os`/`cpu`
during resolution, installs the single package that matches the machine, and skips the
other five without an error. The dispatcher's `bin` entry is a ~40-line Node launcher
that `require.resolve`s the real executable and `spawn`s it.

This is what **esbuild**, **Biome**, **SWC**, **Turborepo**, **Prisma** and **Tailwind's
Oxide engine** all do — none of them are JavaScript at the core either.

The alternative — a `postinstall` script that downloads a binary — is what dev-prune
deliberately does *not* do. It breaks `npm ci --ignore-scripts`, breaks offline and
air-gapped installs, breaks every corporate registry mirror, and turns a dependency
install into an outbound network call to GitHub. Shipping the bytes in the tarball has
none of those failure modes.

**On PyPI**, the mechanism is a platform wheel. A wheel is a zip with a metadata
directory; files placed in `<name>-<version>.data/scripts/` are unpacked straight into
the environment's `bin`/`Scripts` directory at install time. No Python is executed, no
compiler is involved, and no build backend is needed. The wheel's platform tag
(`manylinux_2_17_x86_64`, `win_amd64`, `macosx_11_0_arm64`, …) is what makes pip and uv
select the right one.

The definitive proof that this is normal: **`uv` itself is a Rust binary distributed as
PyPI wheels**, and so is **`ruff`**. `pip install uv` downloads a Rust executable.

`scripts/build_wheels.py` tags the Linux wheels as *both* manylinux and musllinux from
the same asset. That is legitimate rather than a trick — the manylinux policy is a
ceiling on which shared libraries a wheel may require, and a statically linked binary
requires none, so it satisfies the policy trivially while also being the literal
musllinux case. One build serves Debian and Alpine users alike.

What this buys, concretely:

```bash
uv tool install dev-prune     # persistent install, no Python project needed
uvx dev-prune status          # run once, nothing left behind
pipx run dev-prune status     # same, via pipx
npx dev-prune status          # same, via npm — once NPM_PUBLISH is switched on
```

---

## 🚧 Channels that still need a human

The files these channels consume are generated by the release. What is left is the part a
person has to do once: create a repository, or open a pull request someone reviews.

**The manifests themselves are automated.** The `packaging` job in `release.yml` runs
`scripts/render-packaging.sh` against the sidecars the release just published and commits
the result to `main`: a Homebrew formula, a Scoop manifest, and the three WinGet manifest
files. All three are the same three facts — a URL, a SHA-256 and a version — in different
syntaxes, and all three install correctly while being a release out of date, which is why
none of them is maintained by hand. Pre-release tags are skipped: `brew install` and
`scoop install` have no notion of a release channel.

Both by-URL installs work today with no repository and no review:

```bash
brew install https://raw.githubusercontent.com/Life-Experimentalist/dev-prune/main/packaging/homebrew/dev-prune.rb
scoop install https://raw.githubusercontent.com/Life-Experimentalist/dev-prune/main/packaging/scoop/dev-prune.json
```

**The named tap and bucket exist and need nothing from you.**
[`Life-Experimentalist/homebrew-tap`](https://github.com/Life-Experimentalist/homebrew-tap)
and [`Life-Experimentalist/scoop-bucket`](https://github.com/Life-Experimentalist/scoop-bucket)
hold one file each — `Formula/dev-prune.rb` and `bucket/dev-prune.json` — and each has a
`sync.yml` workflow that fetches its file from `packaging/` here every morning and commits
it if it changed. So a release propagates to `brew upgrade` and `scoop update` within a
day of the manifests being refreshed, with no credential anywhere: the sync *pulls* a
public file rather than being pushed a private token. To make it immediate, run the
workflow by hand:

```bash
gh workflow run sync.yml --repo Life-Experimentalist/homebrew-tap
gh workflow run sync.yml --repo Life-Experimentalist/scoop-bucket
```

`homebrew-core` — plain `brew install dev-prune`, no tap prefix — has a real notability
bar and stays a post-popularity step.

**WinGet** is submitted by the release, and merged by a person. `winget-pkgs` has no
popularity requirement, but every version is a pull request a Microsoft reviewer signs
off, so "submitted" and "installable" are days apart. The first submission is open as
[microsoft/winget-pkgs#422665](https://github.com/microsoft/winget-pkgs/pull/422665).

The `submit-winget` job renders the manifests from the published sidecars, strips this
repository's licence header, writes each file as UTF-8 with a BOM, and hands the
directory to `wingetcreate submit`, which forks, branches, commits and opens the pull
request. It needs two things set once:

| Where | Name | Value |
|---|---|---|
| Repository variable | `WINGET_PUBLISH` | `true` |
| Repository secret | `WINGET_TOKEN` | A classic PAT with `public_repo`, owned by the account that forked `winget-pkgs` |

It is gated on the variable rather than on the secret because the `secrets` context is
not readable from a job-level `if:`, and a step that returns early paints the job green.
Until the variable is set, the release summary says **not submitted** in as many words.
The token is passed as `WINGET_CREATE_GITHUB_TOKEN` and never as `--token`, because
winget-create's own documentation warns that the flag can put the token in a log.

Pre-releases are excluded: `winget install` has no notion of a channel, so a `-rc1` tag
would become the version everyone gets.

The first pull request from a given account also asks you to sign Microsoft's CLA, once,
by replying to the bot's comment. That is the one part no token can do.

To submit by hand — a re-submission, or a version whose CI run predates the job:

1. Sync your fork:

   ```bash
   gh repo sync VKrishna04/winget-pkgs --branch master
   ```

2. Produce the submission copies. Do not copy `packaging/winget/*` by hand: the header
   strip and the BOM are exactly what a hand copy gets wrong, and the script CI uses is
   the same one:

   ```bash
   sh scripts/render-packaging.sh <version>
   sh scripts/winget-manifests.sh <fork>/manifests/v/VKrishna04/dev-prune/<version>
   ```

3. Validate before opening anything:

   ```powershell
   winget validate --manifest <the version directory>
   ```

   It must print "Manifest validation succeeded" with no warnings. A warning is exit code
   40 and a failure is 41, so neither is silent. `winget install --manifest` is the other
   half of the check, and needs
   `winget settings --enable LocalManifestFiles` from an elevated prompt first.

4. Open the pull request with the title `New version: VKrishna04.dev-prune version <version>`.

**Chocolatey.** No manifest is generated for it. Its moderation queue is slow enough that it is only worth it if Windows
users ask for it specifically; WinGet covers the same audience with less friction.

**Linux distribution packages** (apt/dnf/AUR). The static musl binary means a `.deb` or
`.rpm` would be trivial to build, but they need a signed repository to be worth
installing from. The one-line installer covers the same ground until someone asks.

---

## 🔥 When a release goes wrong

**A build job failed.** Nothing was published — `publish` needs all six. Fix, then either
delete and re-push the tag (safe only if nobody has fetched it) or re-run the workflow
via Actions with the same tag.

**A publish job failed after another succeeded.** Re-run the workflow with the same tag.
The npm job skips what is already on the registry; `cargo publish` will fail on an
already-published version, which is loud and harmless.

**The tag is wrong.** If nobody has fetched it:

```bash
git tag -d v1.1.0 && git push origin :refs/tags/v1.1.0
```

If anyone might have, cut a new patch version instead. A moved tag is a genuinely nasty
failure for anyone who already pulled it.

**A bad version reached crates.io.** `cargo yank --version 1.1.0` stops new dependents
from resolving it. It cannot be deleted, and the version number cannot be reused. Publish
a fixed patch version.

**A bad version reached npm.** `npm unpublish dev-prune@1.1.0` works within 72 hours if
nothing depends on it. Otherwise `npm deprecate dev-prune@1.1.0 "reason"` and publish a
patch. Remember the seven platform packages need the same treatment.

**A bad version reached PyPI.** Delete the release from the project page. The version
number is burned permanently — publish a patch.

---

## 🔗 See also

- [Multi-Ecosystem Distribution & Packaging Manual](DISTRIBUTION.md) — the user-facing
  view of every install channel
- [GitHub Releases, DIY Manual Install & Source Build](RELEASES_AND_MANUAL_INSTALL.md)
- [`../CONTRIBUTING.md`](../CONTRIBUTING.md) — development setup and the local gate
- [`../CLAUDE.md`](../CLAUDE.md) — repository conventions, including changelog voice
