// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune update`, and the periodic release check behind it.
//
// The check is opt-*out*. An out-of-date cleanup tool is a tool whose safety fixes you do
// not have, so `devp update` asks GitHub for the latest release by default, and `devp
// run` / `devp status` repeat that quietly at most once a week. Both are switched off by
// `devp config set update_check false`, and `devp update --offline` skips a single run.
//
// What leaves the machine is one unauthenticated GET to the public releases endpoint. It
// carries no identifier, no configuration, no repository paths and no usage data — the
// only thing the server learns is that some copy of dev-prune asked what the latest
// version is. Nothing else in the binary opens a socket. See `docs/PRIVACY.md`.
//
// By default the command does not download or install anything: replacing a binary is
// the package manager's job, and doing it ourselves would mean writing to a PATH
// directory with whatever privileges the user happened to have. `--install` keeps that
// division of labour — it works out which package manager owns the running binary and
// runs *that manager's* own upgrade command, rather than writing files itself. The
// scheduled pass is never interrupted by an upgrade: it runs the managed copy under
// `<config>/bin`, which is replaced by atomic rename and refreshed from the new binary
// on the next healthy run (`setup::stable_exe_path`), so a pass already in flight keeps
// its loaded image and the next pass picks up the new one.

use std::cmp::Ordering;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::channel::Channel;
use crate::config::Registry;
use crate::constants;
use crate::output;

pub fn run(offline: bool, install: bool, channels: bool) -> Result<()> {
    if install {
        return run_install();
    }
    if channels {
        return run_channels();
    }
    output::print_header("dev-prune version & upgrade");

    output::print_info(&format!("Installed version: v{}", constants::VERSION));

    let mut registry = Registry::load().ok();

    if offline {
        output::print_info("Skipping the release check because `--offline` was passed.");
    } else if let Some(reg) = registry.as_mut() {
        if reg.settings.update_check {
            // An explicit `devp update` always asks, regardless of when the last
            // automatic check ran — the user is standing there waiting for the answer.
            match refresh_latest(reg) {
                Ok(latest) => report_comparison(&latest),
                // A failed check is not a failed command. Someone offline, behind a
                // proxy, or hitting a rate limit still wants the upgrade instructions.
                Err(e) => output::print_warning(&format!(
                    "Could not reach the release API ({e}). The upgrade commands below still apply."
                )),
            }
            let _ = reg.save();
        } else {
            output::print_info(
                "The release check is off (`devp config set update_check true` re-enables it).",
            );
        }
    }

    println!();
    println!("  Latest releases:  {}", constants::RELEASES_URL);
    println!();

    // Under a pin, the upgrade command is the one command that undoes it. Printing it
    // here would answer "how do I upgrade this" with the wrong answer, so the pin is
    // what the section says instead.
    if registry.is_some_and(|r| r.settings.version_lock) {
        output::print_info(&locked_notice(None));
    } else {
        print_upgrade_commands();
    }

    Ok(())
}

/// The one sentence every refusal prints, so the pin and the way out of it always
/// arrive together.
///
/// A lock that silently does nothing is indistinguishable from an update path that has
/// broken, and "it stopped updating" is what people conclude when a tool goes quiet.
/// `latest` is passed on the paths that already know a newer release exists, because
/// "there is one, and you are deliberately not getting it" is a different fact from
/// "you are pinned".
pub(crate) fn locked_notice(latest: Option<&str>) -> String {
    let head = match latest {
        Some(latest) => format!("dev-prune v{latest} is out. "),
        None => String::new(),
    };
    format!(
        "{head}`version_lock` is on, so this copy stays at v{}. \
         `devp config set version_lock false` releases it.",
        constants::VERSION
    )
}

/// Ask GitHub right now — no interval — and say where the installed build stands.
///
/// For `devp init`, which is deliberate and infrequent enough to be worth a round trip:
/// setting a machine up is exactly the moment to learn the binary is a version behind.
/// `devp run` deliberately does not use this; it goes through [`notify_if_outdated`],
/// which is interval-gated so everyday work never waits on the network.
///
/// Returns `true` when the registry changed and needs saving.
pub fn check_now(registry: &mut Registry) -> bool {
    if !registry.settings.update_check {
        return false;
    }

    match refresh_latest(registry) {
        Ok(latest) => {
            report_comparison(&latest);
            if compare_versions(constants::VERSION, &latest) == Some(Ordering::Less) {
                print_upgrade_commands();
            }
        }
        // Not being able to reach GitHub is not a failed `init`.
        Err(e) => output::print_info(&format!("Could not check for a newer release ({e}).")),
    }
    true
}

/// Name the one command that upgrades *this* copy, and only fall back to the menu.
///
/// The old version printed all eight channels and told the reader to pick the one they
/// installed from. Nobody remembers that — it was a decision made once, possibly a year
/// ago, on a machine they have since reimaged. The channel is written in the path of the
/// running binary and `Channel::detect` already reads it, so asking the user to recall it
/// was asking for information dev-prune already had.
fn print_upgrade_commands() {
    let channel = Channel::detect();
    match channel.upgrade_command() {
        Some(command) => {
            println!("  Installed with {} — upgrade with:", channel.label());
            println!("    {command}");
            println!();
            println!("  Or `devp update --install` to let dev-prune do it for you.");
            println!();
            // Named, not printed. The channel above is the answer for this copy; the
            // rest of the table is for the reader who has a second machine, or who does
            // not believe the detection.
            println!("  `devp update --channels` lists the command for every channel.");
        }
        // `Unknown` means the binary sits somewhere no channel owns — a dev build, a
        // hand-copied file, a distro package. There is no manager to name, so this is the
        // one case where the full list is the honest answer.
        None => {
            println!("  This copy is not in a location any install channel owns, so there");
            println!("  is no package manager to name. Replace it in place with:");
            println!("    devp update --install");
            println!();
            println!("  Or install through a channel, which keeps it upgradeable:");
            print_every_upgrade_command();
        }
    }
}

/// `devp update --channels`: the whole table, and which row this copy is on.
///
/// Deliberately offline. The one question it answers — "what do I type to upgrade a
/// dev-prune installed through X" — does not depend on what the latest release is, and
/// making it wait on the network would make it useless on the machine where it is most
/// often needed.
fn run_channels() -> Result<()> {
    output::print_header("dev-prune upgrade commands");
    let current = Channel::detect();
    println!();
    println!("  This copy came from {}.", current.label());
    println!();
    print_every_upgrade_command();
    println!();
    output::print_info(
        "`devp update --install` replaces this copy directly, without the manager. \
         `devp install --channel <name>` moves it to a different one.",
    );
    Ok(())
}

/// Every channel's own upgrade command, one per line, widest label first.
///
/// Printed from the table rather than typed out. The version of this list that was typed
/// out named five channels of the nine that existed, and the copy the user was holding
/// had been installed through one of the four it did not mention.
fn print_every_upgrade_command() {
    let channels = [
        Channel::Installer,
        Channel::Cargo,
        Channel::Npm,
        Channel::Bun,
        Channel::Pnpm,
        Channel::Yarn,
        Channel::UvTool,
        Channel::Pipx,
        Channel::Pip,
        Channel::WinGet,
        Channel::Scoop,
        Channel::Homebrew,
    ];
    let width = channels.iter().map(|c| c.label().len()).max().unwrap_or(0);
    for channel in channels {
        if let Some(command) = channel.upgrade_command() {
            println!("    {:<width$}  {command}", channel.label());
        }
    }
}

/// `devp update --install`: upgrade this installation to the latest release.
///
/// Downloads the release binary from GitHub and replaces the files itself, rather than
/// asking whichever package manager delivered the first copy to do it. That inversion is
/// deliberate. There is exactly one binary that matters — the managed copy under
/// `<config>/bin`, which the git hooks, the scheduler and `PATH` all point at — and it
/// does not live inside `node_modules`, a uv tool directory or `~/.cargo/bin`. Asking
/// `uv` to upgrade a file it has never heard of was never going to work, and asking it to
/// upgrade its *own* copy left the one that actually runs untouched.
///
/// So both are replaced: the managed copy first, because that is what runs unattended,
/// then the running binary if it is a different file, because that is what the user
/// types. The channel's own bookkeeping (what `uv tool list` believes is installed) is
/// left stale on purpose — correcting it means running the channel's installer, which is
/// the one thing this route exists to avoid — and the command to resync it is printed.
///
/// Falls back to the channel's own upgrade command when there is no published binary for
/// this platform or the download fails, so a release-page outage costs the fast path and
/// not the upgrade.
fn run_install() -> Result<()> {
    output::print_header("dev-prune self-update");

    let mut registry = Registry::load()?;

    // Checked before the network is touched: a refusal the configuration already
    // guarantees should cost nothing and say why.
    if registry.settings.version_lock {
        anyhow::bail!("{}", locked_notice(None));
    }

    if crate::setup::offline_requested() {
        anyhow::bail!(
            "{} is set — an install needs the network by definition.",
            constants::ENV_OFFLINE
        );
    }

    // Know before downloading whether there is anything to download. A failed check is
    // fatal here (unlike `devp update`): running an installer blind would "upgrade" to
    // the version already installed.
    let latest = refresh_latest(&mut registry)?;
    let _ = registry.save();
    if compare_versions(constants::VERSION, &latest) != Some(Ordering::Less) {
        output::print_success(&format!(
            "v{} is already the latest release — nothing to install.",
            constants::VERSION
        ));
        return Ok(());
    }
    output::print_info(&format!("Upgrading v{} -> v{latest} …", constants::VERSION));

    let exe = std::env::current_exe().context("could not locate the running binary")?;
    let managed = crate::setup::managed_exe_path().ok();
    let channel = Channel::detect_at(&exe, managed.as_deref());

    match install_directly(&latest, &exe, managed.as_deref(), channel) {
        Ok(()) => {
            output::print_success(&format!("dev-prune v{latest} installed."));
            report_channel_bookkeeping(channel);
            output::print_info(
                "The scheduled pass was not interrupted: it runs the managed copy, which \
                 was replaced by atomic rename, so a pass already in flight keeps the \
                 image it loaded and the next one picks up the new binary.",
            );
            return Ok(());
        }
        Err(e) => output::print_warning(&format!(
            "Direct download did not work ({e:#}).\nFalling back to the channel that \
             installed this copy."
        )),
    }

    // On Windows a running executable's file is locked against replacement but not
    // against rename. Moving it aside first lets the channel write a fresh file at the
    // real path; the `.old` left behind is swept up by the *next* run, when nothing is
    // executing it any more.
    #[cfg(windows)]
    let aside = {
        let aside = exe.with_extension("exe.old");
        let _ = fs::remove_file(&aside);
        fs::rename(&exe, &aside).ok().map(|_| aside)
    };

    let result = spawn_channel_upgrade(channel);

    #[cfg(windows)]
    if let Some(aside) = aside {
        if result.is_ok() {
            // Best effort: the file is still our running image, so Windows may refuse
            // the delete. The sweep at the top of the next `--install` gets it then.
            let _ = fs::remove_file(&aside);
        } else if !exe.exists() {
            // The upgrade never wrote a new binary — put the old one back so the
            // command the user has on PATH still exists.
            let _ = fs::rename(&aside, &exe);
        }
    }
    result?;

    output::print_success(&format!("dev-prune v{latest} installed."));
    output::print_info(
        "The scheduled pass was not interrupted: it runs the managed copy, which \
         refreshes itself from the new binary on its next run.",
    );
    Ok(())
}

/// Replace every copy of the binary this installation actually runs, from one download.
///
/// The managed copy is done first and is the only one whose failure aborts the upgrade:
/// it is what the scheduler and the git hooks invoke, so a machine with a fresh managed
/// copy is upgraded even if nothing else could be written.
///
/// Every other path is then written from the same verified bytes, and each is written
/// with the same rename-aside dance rather than through `ensure_alias`. That matters on
/// Windows: `ensure_alias` deletes the twin before relinking, and the delete fails when
/// the twin is the running image — which is exactly the case when the user typed `devp
/// update --install`. Renaming a running executable is allowed where deleting it is not,
/// so this route leaves no copy behind on the previous release.
fn install_directly(
    latest: &str,
    exe: &Path,
    managed: Option<&Path>,
    channel: Channel,
) -> Result<()> {
    let bytes = fetch_release_binary(latest)?;
    let primary = managed.unwrap_or(exe);
    install_bytes_at(&bytes, primary)?;

    // …except when a package manager owns the directory the running copy sits in and
    // replaces that directory wholesale on upgrade. Writing new bytes there leaves WinGet,
    // Scoop or Homebrew certain they still have the old version installed, and the next
    // `winget upgrade` puts the old binary back over the top. Their copy is left exactly
    // as the manager wrote it; `report_channel_bookkeeping` names the command that
    // actually moves it forward.
    let replace_exe_dir = primary != exe && exe.is_file() && !channel.replaces_its_directory();
    for path in companion_copies(primary, managed.is_some(), exe, replace_exe_dir) {
        if let Err(e) = install_bytes_at(&bytes, &path) {
            output::print_warning(&format!(
                "The managed copy is now v{latest}, but {} could not be replaced ({e:#}). Until it \
                 is, that copy runs the previous version whenever it is the one invoked.",
                path.display()
            ));
        }
    }

    // The windowless scheduler twin is a *patched* copy, not a plain one, so it is
    // rebuilt rather than written — from the managed binary that was just replaced.
    crate::daemon::refresh_hidden_twin();

    // The receipt beside the managed copy now names the version that was there a minute
    // ago. Only ever updated, never created: this path also upgrades a managed copy some
    // other manager installed, and writing a fresh receipt there would claim one of our
    // installers ran when none did.
    crate::receipt::refresh_after_upgrade(latest);
    Ok(())
}

/// Every other file that is a copy of the binary being replaced — both public names,
/// in both directories that hold one.
///
/// Left alone, a copy keeps running the previous release silently, because the
/// scheduler and the hooks both discard their own output by design. `devp` is a full
/// second executable rather than a link, so a directory that holds one name usually
/// holds the other. The managed directory owns both names outright and gets both
/// written whether they exist yet or not; the running copy's directory belongs to
/// whatever put the binary there, so only files already present are touched.
///
/// Both names, deliberately: through 1.12.0 this list held only the primary's `devp`
/// twin plus the running file itself, so `devp update --install` typed at a
/// cargo-installed `devp` upgraded everything except the `dev-prune` sitting beside
/// it — the exact silent staleness the list exists to prevent.
fn companion_copies(
    primary: &Path,
    primary_is_managed: bool,
    exe: &Path,
    replace_exe_dir: bool,
) -> Vec<PathBuf> {
    let names: [&str; 2] = if cfg!(windows) {
        ["dev-prune.exe", "devp.exe"]
    } else {
        ["dev-prune", "devp"]
    };
    let mut also: Vec<PathBuf> = Vec::new();
    if let Some(dir) = primary.parent() {
        for name in names {
            let twin = dir.join(name);
            if twin != primary && (primary_is_managed || twin.is_file()) {
                also.push(twin);
            }
        }
    }
    if replace_exe_dir {
        if !also.contains(&exe.to_path_buf()) {
            also.push(exe.to_path_buf());
        }
        if let Some(dir) = exe.parent() {
            for name in names {
                let twin = dir.join(name);
                if twin != *exe && twin != primary && twin.is_file() && !also.contains(&twin) {
                    also.push(twin);
                }
            }
        }
    }
    also
}

/// Name the channel's own upgrade command after a direct install, for the one thing the
/// direct route deliberately leaves untouched: the manager's record of what it installed.
fn report_channel_bookkeeping(channel: Channel) {
    // The installer's copy *is* the managed one, and an unrecognised copy has no manager
    // keeping a version record that could disagree with the binary.
    let Some(resync) = channel
        .owns_its_files()
        .then(|| channel.upgrade_command())
        .flatten()
    else {
        return;
    };
    if channel.replaces_its_directory() {
        output::print_info(&format!(
            "The managed copy is now v{}. The copy {} installed was left exactly as it \
             wrote it — replacing a file inside a versioned package directory only makes \
             the manager and the disk disagree. Run `{resync}` to move that one forward \
             too.",
            constants::VERSION,
            channel.label()
        ));
    } else {
        output::print_info(&format!(
            "The binaries are up to date. `{resync}` also updates that manager's own \
             record of the version, which still reads v{}.",
            constants::VERSION
        ));
    }
}

/// Download release `version`'s binary for this platform and put it at `target`.
///
/// The direct route, and the reason `devp update --install` no longer depends on the
/// package manager that happened to deliver the first copy. Whatever installed it, the
/// binary the hooks, the scheduler and `PATH` all point at is one file in the config
/// directory, and this replaces that file. `uv`, `npm` and `cargo` are delivery
/// channels; they are not the source of truth, and asking one of them to upgrade a file
/// living under another one's directory was never going to work.
///
/// Refuses to install anything whose SHA-256 does not match the sidecar published beside
/// it. That check is the entire safety story for this path: the bytes are about to
/// become the binary the machine runs on a schedule.
fn fetch_release_binary(version: &str) -> Result<Vec<u8>> {
    let asset = constants::release_asset_name(version).with_context(|| {
        format!(
            "no published binary for {}-{}; upgrade through the channel that installed \
             this copy instead",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let base = format!("{}/v{version}/{asset}", constants::RELEASE_DOWNLOAD_BASE);

    let expected = fetch_expected_hash(&format!("{base}.sha256"))?;
    output::print_info(&format!("Downloading {asset} …"));
    let bytes = fetch_bytes(&base)?;

    let actual = {
        use sha2::{Digest, Sha256};
        use std::fmt::Write as _;
        let mut h = Sha256::new();
        h.update(&bytes);
        // Hex-encoded by hand: sha2 0.11 returns a `hybrid_array::Array`, which has no
        // `LowerHex`, and the sidecar is lower-case hex either way.
        h.finalize().iter().fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
    };
    if actual != expected {
        anyhow::bail!(
            "checksum mismatch for {asset}\n  expected {expected}\n  got      {actual}\n\
             The download was corrupted or tampered with; nothing was installed."
        );
    }

    Ok(bytes)
}

/// Write already-verified bytes over one binary.
///
/// Separate from the download so a single transfer can serve every copy that has to be
/// replaced — the managed binary, its `devp` twin, and whatever the user is running —
/// instead of fetching the same megabytes once per path.
fn install_bytes_at(bytes: &[u8], target: &Path) -> Result<()> {
    // Staged beside the target and renamed in, so a write that dies half-way leaves the
    // working binary untouched rather than a truncated file where the scheduler expects
    // an executable.
    let staging = target.with_extension("new");
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&staging, bytes).with_context(|| format!("could not write {}", staging.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Downloaded files are 0644; the scheduler needs to be able to run this.
        let _ = fs::set_permissions(&staging, fs::Permissions::from_mode(0o755));
    }

    replace_binary(&staging, target)
}

/// Read the hash out of a `.sha256` sidecar published beside a release asset.
fn fetch_expected_hash(url: &str) -> Result<String> {
    let body = String::from_utf8(fetch_bytes(url)?).context("the checksum sidecar was not text")?;
    parse_sha256_sidecar(&body)
}

/// The parsing half of [`fetch_expected_hash`], which is `sha256sum` format: the hex
/// digest, two spaces, the file name.
///
/// Validated rather than trusted, because the failure this guards against is not a
/// malformed checksum — it is a 404 page or a proxy error blob arriving where the sidecar
/// should be. Comparing a digest against `<!DOCTYPE html>` would report a checksum
/// mismatch, which reads as "someone tampered with the download" and sends the user
/// somewhere alarming and wrong.
fn parse_sha256_sidecar(body: &str) -> Result<String> {
    let hash = body
        .split_whitespace()
        .next()
        .context("the checksum sidecar was empty")?
        .to_ascii_lowercase();
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        anyhow::bail!("the checksum sidecar did not contain a SHA-256 digest");
    }
    Ok(hash)
}

fn fetch_bytes(url: &str) -> Result<Vec<u8>> {
    let mut body = ureq::get(url)
        .header("User-Agent", &format!("dev-prune/{}", constants::VERSION))
        .config()
        .timeout_global(Some(Duration::from_secs(
            constants::UPDATE_DOWNLOAD_TIMEOUT_SECS,
        )))
        .build()
        .call()
        .with_context(|| format!("could not download {url}"))?;
    let mut buf = Vec::new();
    body.body_mut()
        .as_reader()
        .read_to_end(&mut buf)
        .with_context(|| format!("could not read {url}"))?;
    Ok(buf)
}

/// Move `staged` onto `target`, working around the one platform that will not overwrite
/// a file it is executing.
fn replace_binary(staged: &Path, target: &Path) -> Result<()> {
    // On Windows a running image is locked against replacement but not against rename,
    // so the live file steps aside and the new one takes its name. The `.old` is swept
    // by the next run, when nothing holds it open any more.
    #[cfg(windows)]
    let aside = {
        let aside = target.with_extension("exe.old");
        let _ = fs::remove_file(&aside);
        target
            .exists()
            .then(|| fs::rename(target, &aside).ok().map(|_| aside))
            .flatten()
    };

    match fs::rename(staged, target) {
        Ok(()) => {
            #[cfg(windows)]
            if let Some(aside) = aside {
                let _ = fs::remove_file(&aside);
            }
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(staged);
            #[cfg(windows)]
            if let Some(aside) = aside
                && !target.exists()
            {
                // Put the working binary back rather than leaving the machine with no
                // `dev-prune` at all.
                let _ = fs::rename(&aside, target);
            }
            Err(e).with_context(|| format!("could not install {}", target.display()))
        }
    }
}

/// Run one channel's own upgrade command, wired to the terminal so its progress and
/// prompts reach the user directly.
fn spawn_channel_upgrade(channel: Channel) -> Result<()> {
    let Some(argv) = channel.upgrade_argv() else {
        output::print_warning(
            "Could not tell which channel installed this binary, so nothing was \
             changed. Upgrade it yourself with one of:",
        );
        print_upgrade_commands();
        anyhow::bail!("unrecognised install channel");
    };

    output::print_info(&format!("Running: {}", argv.join(" ")));
    let status = crate::spawn::command(crate::adapters::resolve_program(&argv[0]))
        .args(&argv[1..])
        .status()
        .with_context(|| format!("could not start `{}`", argv[0]))?;
    if !status.success() {
        anyhow::bail!("`{}` exited with {status}", argv.join(" "));
    }
    Ok(())
}

/// The end-of-run hook behind `auto_update`: when the setting is on and the last release
/// check already knows a newer version exists, replace the binary without being asked.
///
/// Warn-never-fail, like everything else that runs as a side effect of `devp run` — a
/// broken upgrade path must not turn a successful prune into a failed command.
///
/// Deliberately *not* `run_install`. That function falls back to running the package
/// manager that installed this copy, and this is the path that runs unattended: from the
/// scheduled pass, from a git hook, from `devp run` in the middle of someone else's
/// work. Spawning `winget upgrade` there can raise an elevation prompt and can pull in
/// upgrades nobody asked about. Download-and-replace is safe unattended; handing the
/// machine to a package manager is a decision, and decisions stay with the person.
pub fn maybe_auto_update(registry: &Registry) {
    if !registry.settings.auto_update
        || crate::setup::offline_requested()
        || crate::setup::no_auto_setup_requested()
    {
        return;
    }
    let Some(latest) = registry.latest_known_version.as_deref() else {
        return;
    };
    if compare_versions(constants::VERSION, latest) != Some(Ordering::Less) {
        return;
    }

    // Announced here rather than at the top of the function, so the line appears on
    // exactly the runs where the pin changed the outcome. A pass with nothing to
    // install stays as silent as it has always been.
    if registry.settings.version_lock {
        println!();
        output::print_info(&locked_notice(Some(latest)));
        return;
    }

    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let managed = crate::setup::managed_exe_path().ok();
    let channel = Channel::detect_at(&exe, managed.as_deref());

    // WinGet, Scoop and Homebrew swap their whole package directory on upgrade, so bytes
    // written there are undone by the next `winget upgrade` — which would still believe
    // the old version is installed. Those channels own the upgrade, and
    // `notify_if_outdated` has already printed the line naming the right command.
    if channel.replaces_its_directory() {
        return;
    }

    println!();
    output::print_info(&format!(
        "Updating dev-prune v{} -> v{latest} …",
        constants::VERSION
    ));
    match install_directly(latest, &exe, managed.as_deref(), channel) {
        Ok(()) => {
            output::print_success(&format!("dev-prune v{latest} installed."));
            report_channel_bookkeeping(channel);
        }
        Err(e) => output::print_warning(&format!(
            "Automatic update failed ({e:#}). Run `devp update --install` yourself, or \
             `devp config set auto_update false` to stop trying."
        )),
    }
}

/// Quietly keep the release check current and print a one-line notice when the installed
/// build is behind. Returns `true` when the registry changed and needs saving.
///
/// Called from `devp run` and `devp status`. Never returns an error: a background
/// convenience must not be able to fail the command the user actually asked for.
pub fn notify_if_outdated(registry: &mut Registry) -> bool {
    if !registry.settings.update_check {
        return false;
    }

    let interval = registry.settings.update_check_interval_days;
    let due = registry
        .last_update_check
        .is_none_or(|last| Utc::now().signed_duration_since(last).num_days() >= interval);

    if due {
        // The result is deliberately ignored: `refresh_latest` moves the timestamp even
        // when the request fails, and retrying on every command while the machine is
        // offline would put a five-second stall in front of everyday work.
        let _ = refresh_latest(registry);
    }

    if let Some(latest) = registry.latest_known_version.as_deref()
        && compare_versions(constants::VERSION, latest) == Some(Ordering::Less)
    {
        if registry.settings.version_lock {
            output::print_info(&locked_notice(Some(latest)));
        } else {
            output::print_info(&format!(
                "dev-prune v{latest} is out (you have v{}). `devp update` has the commands; \
                 `devp config set update_check false` silences this.",
                constants::VERSION
            ));
        }
    }

    due
}

/// Ask GitHub for the latest release and record the answer on the registry.
///
/// The caller is responsible for saving; that keeps this usable from both the
/// already-loaded-registry path and the standalone command.
fn refresh_latest(registry: &mut Registry) -> Result<String> {
    let result = latest_release(registry.settings.update_check_timeout_secs);
    registry.last_update_check = Some(Utc::now());
    let latest = result?;
    registry.latest_known_version = Some(latest.clone());
    Ok(latest)
}

/// Say whether the installed build is behind, current, or ahead of the latest release.
fn report_comparison(latest: &str) {
    let installed = constants::VERSION;
    match compare_versions(installed, latest) {
        Some(Ordering::Less) => {
            output::print_warning(&format!(
                "Latest release:    v{latest} — an upgrade is available."
            ));
        }
        Some(Ordering::Equal) => {
            output::print_success(&format!(
                "Latest release:    v{latest} — you are up to date."
            ));
        }
        Some(Ordering::Greater) => {
            // Normal when running a local build between releases.
            output::print_info(&format!(
                "Latest release:    v{latest} — your build is newer than the last published one."
            ));
        }
        None => {
            output::print_info(&format!(
                "Latest release:    v{latest} (could not compare it to v{installed})."
            ));
        }
    }
}

/// Fetch the tag name of the most recent published release.
///
/// Returns the version without any leading `v`, so it can be compared to
/// `CARGO_PKG_VERSION` directly.
fn latest_release(timeout_secs: u64) -> Result<String> {
    if crate::setup::offline_requested() {
        anyhow::bail!("{} is set", constants::ENV_OFFLINE);
    }
    let body = ureq::get(constants::LATEST_RELEASE_API_URL)
        .header("User-Agent", &format!("dev-prune/{}", constants::VERSION))
        .header("Accept", "application/vnd.github+json")
        .config()
        .timeout_global(Some(Duration::from_secs(timeout_secs.max(1))))
        .build()
        .call()
        .context("request failed")?
        .body_mut()
        .read_to_string()
        .context("could not read the response")?;

    let json: serde_json::Value =
        serde_json::from_str(&body).context("the response was not JSON")?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .context("the response carried no tag_name")?;

    Ok(tag.trim_start_matches('v').to_string())
}

/// Compare two dotted numeric versions, ignoring any pre-release suffix.
///
/// Returns `None` when either side is not `major.minor.patch` — better to say "could not
/// compare" than to claim an upgrade exists because `1.0.0` sorts before `1.0.0-rc.1`
/// as a string.
pub(crate) fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    let parse = |v: &str| -> Option<[u64; 3]> {
        let core = v.split(['-', '+']).next()?;
        let mut parts = core.split('.');
        let out = [
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ];
        // A fourth component means this is not the scheme we release under.
        if parts.next().is_some() {
            return None;
        }
        Some(out)
    };
    Some(parse(a)?.cmp(&parse(b)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    #[test]
    fn orders_by_component_not_lexically() {
        // "1.10.0" < "1.9.0" as strings, which is the bug this function exists to avoid.
        assert_eq!(compare_versions("1.9.0", "1.10.0"), Some(Ordering::Less));
        assert_eq!(compare_versions("1.0.0", "1.0.0"), Some(Ordering::Equal));
        assert_eq!(
            compare_versions("2.0.0", "1.99.99"),
            Some(Ordering::Greater)
        );
    }

    #[test]
    fn pre_release_suffixes_compare_by_their_core() {
        assert_eq!(
            compare_versions("1.0.0", "1.0.0-rc.1"),
            Some(Ordering::Equal)
        );
        assert_eq!(
            compare_versions("1.0.0+build7", "1.0.1"),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn unparseable_versions_report_no_answer_rather_than_a_wrong_one() {
        assert_eq!(compare_versions("1.0", "1.0.0"), None);
        assert_eq!(compare_versions("1.0.0.1", "1.0.0"), None);
        assert_eq!(compare_versions("nightly", "1.0.0"), None);
    }

    fn bin_names() -> [&'static str; 2] {
        if cfg!(windows) {
            ["dev-prune.exe", "devp.exe"]
        } else {
            ["dev-prune", "devp"]
        }
    }

    #[test]
    fn an_update_reaches_the_twin_beside_the_copy_the_user_typed() {
        // The 1.12.0 bug: `devp update --install` typed at a cargo-installed `devp`
        // upgraded the managed pair and the running file, and left the `dev-prune`
        // beside it on the previous release.
        let [prune, devp] = bin_names();
        let managed_dir = tempfile::tempdir().unwrap();
        let cargo_dir = tempfile::tempdir().unwrap();
        let primary = managed_dir.path().join(prune);
        let exe = cargo_dir.path().join(devp);
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(cargo_dir.path().join(prune), b"old").unwrap();

        let also = companion_copies(&primary, true, &exe, true);
        assert!(also.contains(&managed_dir.path().join(devp)));
        assert!(also.contains(&exe));
        assert!(also.contains(&cargo_dir.path().join(prune)));
        assert!(!also.contains(&primary));
    }

    #[test]
    fn a_name_that_does_not_exist_outside_the_managed_directory_is_not_invented() {
        // The managed directory owns both names; the running copy's directory belongs
        // to whatever installed it, so a missing twin there stays missing.
        let [prune, devp] = bin_names();
        let managed_dir = tempfile::tempdir().unwrap();
        let solo_dir = tempfile::tempdir().unwrap();
        let primary = managed_dir.path().join(prune);
        let exe = solo_dir.path().join(devp);
        std::fs::write(&exe, b"old").unwrap();

        let also = companion_copies(&primary, true, &exe, true);
        assert!(also.contains(&managed_dir.path().join(devp)));
        assert!(also.contains(&exe));
        assert!(!also.contains(&solo_dir.path().join(prune)));
    }

    #[test]
    fn a_manager_owned_directory_is_left_exactly_as_the_manager_wrote_it() {
        // replace_exe_dir is false for WinGet/Scoop/Homebrew copies; nothing in the
        // running copy's directory may be rewritten, twins included.
        let [prune, devp] = bin_names();
        let managed_dir = tempfile::tempdir().unwrap();
        let store_dir = tempfile::tempdir().unwrap();
        let primary = managed_dir.path().join(prune);
        let exe = store_dir.path().join(devp);
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(store_dir.path().join(prune), b"old").unwrap();

        let also = companion_copies(&primary, true, &exe, false);
        assert_eq!(also, vec![managed_dir.path().join(devp)]);
    }

    #[test]
    fn without_a_managed_copy_only_existing_files_beside_the_binary_are_replaced() {
        let [prune, devp] = bin_names();
        let dir = tempfile::tempdir().unwrap();
        let primary = dir.path().join(devp);
        std::fs::write(&primary, b"old").unwrap();

        // Alone, its absent `dev-prune` twin is not created…
        assert!(companion_copies(&primary, false, &primary, false).is_empty());

        // …but a twin that exists is stale the moment the primary is replaced.
        std::fs::write(dir.path().join(prune), b"old").unwrap();
        assert_eq!(
            companion_copies(&primary, false, &primary, false),
            vec![dir.path().join(prune)]
        );
    }

    #[test]
    fn the_check_is_on_unless_the_user_turns_it_off() {
        assert!(Registry::default().settings.update_check);
    }

    #[test]
    fn a_disabled_check_touches_neither_the_network_nor_the_registry() {
        let mut registry = Registry::default();
        registry.settings.update_check = false;
        assert!(!notify_if_outdated(&mut registry));
        assert!(registry.last_update_check.is_none());
    }

    #[test]
    fn auto_update_is_on_by_default_and_silent_with_nothing_to_install() {
        let registry = Registry::default();
        assert!(registry.settings.auto_update);
        // No release check has run, so `latest_known_version` is unset and this must
        // return without touching the network or the terminal. The default being *on* is
        // what makes that early return load-bearing rather than incidental.
        assert!(registry.latest_known_version.is_none());
        maybe_auto_update(&registry);
    }

    #[test]
    fn the_pin_is_off_until_somebody_asks_for_it() {
        // Every other path in this file is written on the assumption that the pin costs
        // nothing when nobody has set it, so the default is the part worth asserting.
        assert!(!Registry::default().settings.version_lock);
    }

    #[test]
    fn the_refusal_names_the_version_it_is_holding_and_the_way_out() {
        // Both halves matter. A refusal that does not say which version it is holding
        // cannot be audited, and one that does not say how to release it is
        // indistinguishable, to the person reading it, from an update path that broke.
        let notice = locked_notice(None);
        assert!(notice.contains(constants::VERSION), "{notice}");
        assert!(
            notice.contains("devp config set version_lock false"),
            "{notice}"
        );
        assert!(!notice.contains("is out"), "{notice}");
    }

    #[test]
    fn a_known_release_is_named_in_the_refusal_that_withholds_it() {
        // "You are pinned" and "there is a 2.0.0 out that you are not getting" are
        // different facts, and the second is the one that makes somebody go and look at
        // the setting.
        let notice = locked_notice(Some("2.0.0"));
        assert!(notice.contains("v2.0.0 is out"), "{notice}");
        assert!(notice.contains(constants::VERSION), "{notice}");
    }

    #[test]
    fn a_recent_check_is_not_repeated() {
        let mut registry = Registry::default();
        let stamp = Utc::now() - ChronoDuration::days(constants::UPDATE_CHECK_INTERVAL_DAYS - 1);
        registry.last_update_check = Some(stamp);
        // No network call, so the stamp survives untouched and nothing needs saving.
        assert!(!notify_if_outdated(&mut registry));
        assert_eq!(registry.last_update_check, Some(stamp));
    }

    #[test]
    fn the_asset_name_matches_what_the_release_workflow_builds() {
        // This string is a contract with `.github/workflows/release.yml`. Getting it
        // wrong is not a compile error and not a test failure anywhere else — it is a
        // self-update that 404s for every user on the day of a release.
        let name = constants::release_asset_name("1.4.0");
        let expected = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", "x86_64") => Some("dev-prune-v1.4.0-windows-x64.exe"),
            ("windows", "aarch64") => Some("dev-prune-v1.4.0-windows-arm64.exe"),
            ("windows", "x86") => Some("dev-prune-v1.4.0-windows-x86.exe"),
            ("linux", "x86_64") => Some("dev-prune-v1.4.0-linux-x64"),
            ("linux", "aarch64") => Some("dev-prune-v1.4.0-linux-arm64"),
            ("macos", "x86_64") => Some("dev-prune-v1.4.0-darwin-x64"),
            ("macos", "aarch64") => Some("dev-prune-v1.4.0-darwin-arm64"),
            // A platform the release does not build for must decline the direct route
            // rather than download some other platform's binary.
            _ => None,
        };
        assert_eq!(name.as_deref(), expected);
    }

    #[test]
    fn only_windows_has_a_32_bit_asset() {
        // The matrix builds `x86` for Windows alone. On a 32-bit Linux there is nothing
        // to download, and guessing `x64` would install a binary that cannot run.
        let name = constants::release_asset_name("9.9.9");
        if std::env::consts::ARCH == "x86" {
            assert_eq!(name.is_some(), std::env::consts::OS == "windows");
        }
    }

    #[test]
    fn a_sidecar_is_read_as_the_first_field_of_sha256sum_format() {
        let digest = "a".repeat(64);
        assert_eq!(
            parse_sha256_sidecar(&format!("{digest}  dev-prune-v1.4.0-linux-x64\n")).unwrap(),
            digest
        );
        // The Windows step writes it with no trailing newline, and GitHub may serve
        // either line ending.
        assert_eq!(
            parse_sha256_sidecar(&format!("{digest}  asset.exe")).unwrap(),
            digest
        );
        assert_eq!(
            parse_sha256_sidecar(&format!("{}  asset\r\n", digest.to_uppercase())).unwrap(),
            digest,
            "an upper-case digest must compare equal to the one we compute"
        );
    }

    #[test]
    fn anything_that_is_not_a_digest_is_refused_before_it_is_compared() {
        // A 404 page, an error blob or a truncated read must fail as "not a digest"
        // rather than as a mismatch — the two send the user to very different places.
        for bad in [
            "",
            "   ",
            "<!DOCTYPE html>",
            "not-a-hash  asset",
            &"a".repeat(63),
            &"a".repeat(65),
            &format!("{}g  asset", "a".repeat(63)),
        ] {
            assert!(
                parse_sha256_sidecar(bad).is_err(),
                "{bad:?} must not be accepted as a digest"
            );
        }
    }
}
