# Privacy & Network Policy

dev-prune collects no analytics, no diagnostics and no usage data — not locally, not
remotely, not in aggregate, not anonymised. There is no opt-out to configure, because
there is nothing to opt out of.

It does make exactly one network request on its own initiative: asking GitHub what the
latest release is. Two others exist and neither happens unless you ask for it —
downloading a release binary when you run `devp update --install`, and downloading the
VS Code extension's `.vsix` after you say yes to the one-time editor-extension offer.
All three are described below.

---

## The one request

| | |
|---|---|
| **Endpoint** | `https://api.github.com/repos/Life-Experimentalist/dev-prune/releases/latest` |
| **Method** | `GET`, unauthenticated |
| **Headers sent** | `User-Agent: dev-prune/<version>`, `Accept: application/vnd.github+json` |
| **Body sent** | none |
| **Timeout** | 5 seconds |
| **Frequency** | on `devp update`; at most once every 7 days from `devp run` and `devp status` |
| **Turn it off** | `devp config set update_check false` |

The only fact this reveals to GitHub is that *some* copy of dev-prune at *some* version
asked what the newest release is. No repository paths, no repository names, no directory
listings, no machine identifier, no configuration, no counters, and no persistent ID are
sent — the request has no body and no cookie to carry them in. As with any HTTP request,
GitHub sees the connecting IP address; dev-prune neither adds to that nor can prevent it.

The answer is cached in the registry (`last_update_check`, `latest_known_version`) so the
reminder survives without going back to the network.

### Why it is on by default

A cleanup tool that deletes directories is a tool whose bug fixes you want. Shipping the
check off-by-default means the users least likely to hear about a safety fix are the ones
who never find the flag. So it is opt-**out**, and it is a single line to turn off:

```bash
devp config set update_check false
```

`devp update --offline` skips it for one invocation without changing the setting, and
setting the environment variable `DEV_PRUNE_OFFLINE=1` keeps the process off the network
entirely — this check and the `.vsix` fallback below alike — regardless of any setting.

### Why there is no auto-update

dev-prune tells you a newer version exists and prints the upgrade command for your
install channel. It does not download or replace its own binary. Doing that would mean
writing to a directory on `PATH` with whatever privileges the user happened to have, and
fetching an executable over a channel with no signature verification of its own. Your
package manager already does this correctly; dev-prune defers to it.

---

## The download, if you run `devp update --install`

`devp update --install` fetches the release binary for this platform, and the SHA-256
sidecar published beside it, from GitHub's release-download host.

| | |
|---|---|
| **Endpoint** | `https://github.com/Life-Experimentalist/dev-prune/releases/download/v<version>/<asset>` and the same URL plus `.sha256` |
| **Method** | `GET`, unauthenticated |
| **Headers sent** | `User-Agent: dev-prune/<version>` |
| **Body sent** | none |
| **Timeout** | 300 seconds |
| **Frequency** | only when you run `devp update --install` — or, if you switched `auto_update` on yourself, at the end of a prune pass that already knows a newer release exists |
| **Turn it off** | don't run it; `DEV_PRUNE_OFFLINE=1` refuses it outright |

The asset name encodes the operating system and CPU architecture, because that is which
file to send — it is the same URL a person would click on the release page, and it
carries no identifier of any kind. The download is rejected unless its SHA-256 matches
the sidecar, and nothing is written when it does not match.

---

## The extension install, if you say yes

When `devp setup` finds a VS Code-family editor, it asks — once, ever, and only
interactively — whether to install the dev-prune editor extension. Saying yes runs the
editor's own `--install-extension` command; any download that triggers is the editor's
traffic against its own marketplace, not dev-prune's. If the editor's registry cannot
resolve the extension (some forks' registries do not carry it), dev-prune itself
downloads the `.vsix` attached to the latest GitHub release — the same
`api.github.com/...releases` endpoint as the update check, same headers, no body — and
hands that file to the editor. This never happens without the explicit confirmation,
and a "no" is remembered: nothing asks twice.

---

## Everything else stays on the machine

| Data | Where it lives | Ever transmitted |
|---|---|---|
| Registry of tracked repositories | `<config dir>/registry.json` | No |
| Per-repository settings | `.devprune.json` in each repository | No |
| Prune history and byte counters | `<config dir>/registry.json` | No |
| Daemon logs | `<config dir>/` | No |
| Directory sizes and scan results | memory only | No |

`<config dir>` is `%APPDATA%\dev-prune` on Windows and `~/.config/dev-prune` elsewhere,
overridable with `DEV_PRUNE_CONFIG_DIR`.

## Subprocesses dev-prune runs

Restoring a pruned directory means running the ecosystem's own installer — `npm ci`,
`uv sync`, `cargo fetch` and so on. Those commands make their own network requests to
their own registries, governed by their own configuration and their own privacy policies.
dev-prune invokes them; it does not proxy, inspect or alter their traffic.

This only happens when you ask for it: `devp restore`, or the lockfile verification step
of a prune when the two-tier check needs to prove the lockfile can rebuild what is about
to be deleted.

## Verifying this yourself

The claim is checkable rather than promised:

```bash
# The only URL constant in the binary.
grep -rn "https://" src/constants.rs

# The only module that uses the HTTP client.
grep -rln "ureq" src/
```

`ureq` is the sole dependency in `Cargo.toml` capable of opening a socket, and only two
files call it: `src/commands/update.rs` (the release check) and `src/setup.rs` (the
user-confirmed `.vsix` download above). Nothing else in the binary can reach the
network.
