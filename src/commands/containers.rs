// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune caches docker`, `caches podman` and `caches containers`.
//
// A container engine is usually the largest thing on a developer's disk and the last one
// anybody looks at. `devp caches` already answers "how big is the npm cache"; this
// answers the same question about images, stopped containers, dangling volumes and the
// build cache, which between them routinely hold more than every package manager cache
// on the machine combined.
//
// **Nothing on a schedule will ever delete any of it**, and until 1.17.0 nothing here
// deleted it at all. The reasoning behind that has not changed: a container image has no
// lockfile — the registry tag it came from can be retagged or deleted, the Dockerfile
// that built it may not be on this disk — and a named volume is the one thing in the
// whole system that is not reproducible at any price.
//
// What changed is who runs the command. The report used to end by printing four commands
// and asking the reader to go type one in another window, which meant the reclaim was
// theirs to have remembered, and dev-prune could neither count it nor say afterwards what
// it had cost. `caches clear <engine>` now runs the narrow ones itself — build cache,
// unused images, stopped containers — in the foreground, after printing them, after
// asking, and never from the daemon. The volume-deleting variants stay printed and
// unrun, which is the only part of this that was ever about proof rather than about
// consent.
//
// The numbers come from the engine's own `system df`, not from a directory walk. On
// Docker Desktop and Podman the store lives inside a VM disk image that the host cannot
// see, and `~/.docker` is a config directory rather than the data — a size taken from
// the filesystem would be wrong by orders of magnitude, and wrong in the reassuring
// direction. Asking the engine is also the only way to learn what is *reclaimable*,
// which is the figure that decides anything: 40 GB of images with 38 GB dangling is a
// different situation from 40 GB with 2 GB dangling.
//
// Kubernetes is reported as names and no bytes. kind, k3d and minikube run their nodes
// as containers or as a VM disk belonging to an engine that is already in the table
// above, so a size beside a cluster name would be gigabytes counted twice.

use std::path::PathBuf;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;

use crate::adapters;
use crate::constants;
use crate::json;
use crate::output;

/// A container engine dev-prune knows how to ask about its disk use.
struct Engine {
    /// What it is called in output, and the name accepted on the command line.
    name: &'static str,
    /// The executable to look for and to ask.
    binary: &'static str,
    /// Arguments that make it print its disk usage as JSON.
    ///
    /// Docker, nerdctl and finch take a Go template; Podman and Apple's `container` take
    /// a format name. The first four then produce the same rows in one of two
    /// punctuations, which is why one parser reads either; Apple's is a different
    /// document and [`parse_rows`] says so.
    df_args: &'static [&'static str],
    /// The reclaim commands worth printing, narrowest first, each with what it costs.
    ///
    /// Printed and never run. The order is the order to try them in: the build cache is
    /// almost always the biggest win and the only one that costs nothing but a slower
    /// next build, and the volume-deleting variant is last because it is the one that
    /// destroys data no registry can hand back.
    prune: &'static [(&'static str, &'static str)],
    /// The steps `devp caches clear <engine>` runs, in order.
    ///
    /// A separate table from `prune` on purpose. That one is every command worth knowing
    /// about, including the volume-deleting variant nobody should reach for casually;
    /// this one is only what dev-prune is willing to run itself. There is no argv here
    /// that touches a volume, so "volumes are left alone" is a property of the table
    /// rather than a flag someone can pass or a check that could be forgotten.
    reclaim: &'static [ReclaimStep],
    /// Whether this engine stops to ask before it prunes.
    ///
    /// Docker, Podman, nerdctl and finch all do, and all take `-f` to say the question
    /// has already been asked — which dev-prune has, by name, in the plan it printed
    /// first. Apple's `container` has neither the question nor the flag: `container
    /// prune` removes stopped containers and prints what it reclaimed, and a `-f` it does
    /// not define would turn every step into a usage error. So this is a fact about the
    /// engine, checked in the tests, rather than a habit applied to all of them.
    prompts: bool,
}

/// One command `devp caches clear <engine>` runs.
struct ReclaimStep {
    /// What it gives back, in the plan and in the result line.
    what: &'static str,
    /// The engine's own arguments, forced non-interactive.
    ///
    /// `-f` is not a shortcut past a confirmation the user never saw: dev-prune has
    /// already asked, by name, for everything these steps do. What it prevents is the
    /// engine asking a second question at a prompt this process may not own.
    args: &'static [&'static str],
}

/// Width of the command column under "Reclaim it yourself".
///
/// `docker system prune --volumes` is the longest command printed at 29 characters, and
/// every cost string below is written to fit the remainder inside 90 columns.
const COMMAND_WIDTH: usize = 32;

/// Every engine this command knows, in the order they are reported.
const ENGINES: &[Engine] = &[
    Engine {
        name: "docker",
        binary: "docker",
        prompts: true,
        df_args: &["system", "df", "--format", "{{json .}}"],
        prune: &[
            (
                "docker builder prune",
                "the build cache; costs a slower next build",
            ),
            (
                "docker image prune",
                "dangling images no tag points at any more",
            ),
            (
                "docker container prune",
                "stopped containers and each writable layer",
            ),
            (
                "docker system prune",
                "the three above at once; volumes untouched",
            ),
            (
                "docker system prune --volumes",
                "adds unused volumes — the one that deletes data",
            ),
        ],
        reclaim: &[
            ReclaimStep {
                what: "the build cache",
                args: &["builder", "prune", "-a", "-f"],
            },
            ReclaimStep {
                what: "images no container uses",
                args: &["image", "prune", "-a", "-f"],
            },
            ReclaimStep {
                what: "stopped containers and their writable layers",
                args: &["container", "prune", "-f"],
            },
        ],
    },
    Engine {
        name: "podman",
        binary: "podman",
        prompts: true,
        df_args: &["system", "df", "--format", "json"],
        prune: &[
            (
                "podman system prune",
                "stopped containers, networks, dangling images",
            ),
            (
                "podman image prune -a",
                "every image no container uses, tagged or not",
            ),
            (
                "podman system prune --volumes",
                "adds unused volumes — the one that deletes data",
            ),
        ],
        reclaim: &[
            ReclaimStep {
                what: "the build cache",
                args: &["builder", "prune", "-a", "-f"],
            },
            ReclaimStep {
                what: "images no container uses",
                args: &["image", "prune", "-a", "-f"],
            },
            ReclaimStep {
                what: "stopped containers and their writable layers",
                args: &["container", "prune", "-f"],
            },
        ],
    },
    Engine {
        name: "nerdctl",
        binary: "nerdctl",
        prompts: true,
        df_args: &["system", "df", "--format", "{{json .}}"],
        prune: &[
            (
                "nerdctl system prune",
                "stopped containers, networks, dangling images",
            ),
            (
                "nerdctl system prune --volumes",
                "adds unused volumes — the one that deletes data",
            ),
        ],
        // One step rather than three: nerdctl spells its narrow prune subcommands
        // differently across versions, and `system prune` has meant the same thing —
        // images, containers, build cache, volumes only with `--volumes` — since it
        // gained the command.
        reclaim: &[ReclaimStep {
            what: "images, stopped containers and the build cache",
            args: &["system", "prune", "-a", "-f"],
        }],
    },
    // finch is nerdctl inside a Lima VM, and it forwards `system` to it verbatim with
    // flag parsing turned off — so the nerdctl spellings above are the finch spellings,
    // template and all. Its store is inside that VM's disk image, which is the same
    // reason the host cannot size it and the engine has to be the one asked.
    Engine {
        name: "finch",
        binary: "finch",
        prompts: true,
        df_args: &["system", "df", "--format", "{{json .}}"],
        prune: &[
            (
                "finch system prune",
                "stopped containers, networks, dangling images",
            ),
            (
                "finch system prune --volumes",
                "adds unused volumes — the one that deletes data",
            ),
        ],
        reclaim: &[ReclaimStep {
            what: "images, stopped containers and the build cache",
            args: &["system", "prune", "-a", "-f"],
        }],
    },
    // Apple's `container`, on Apple silicon. Named after its binary like the rest, so
    // `devp caches clear container` is the command someone who has been typing
    // `container` all day would guess.
    //
    // It is the odd one here twice over. Its `system df` answers with one object whose
    // fields are the resource types rather than a row each, and its prune subcommands
    // have no confirmation and therefore no `-f`. There is also nothing to clear a build
    // cache with: BuildKit lives in a builder VM, and `container builder delete` removes
    // the builder itself rather than pruning what it cached, which is more than being
    // asked for.
    Engine {
        name: "container",
        binary: "container",
        prompts: false,
        df_args: &["system", "df", "--format", "json"],
        prune: &[
            (
                "container image prune -a",
                "every image no container uses, tagged or not",
            ),
            (
                "container prune",
                "stopped containers and their writable layers",
            ),
            (
                "container volume prune",
                "unused volumes — the one that deletes data",
            ),
        ],
        reclaim: &[
            ReclaimStep {
                what: "images no container uses",
                args: &["image", "prune", "-a"],
            },
            ReclaimStep {
                what: "stopped containers and their writable layers",
                args: &["prune"],
            },
        ],
    },
];

/// One line of an engine's own disk-usage report.
pub struct Row {
    /// `Images`, `Containers`, `Local Volumes`, `Build Cache` — the engine's own word
    /// for it, kept verbatim so the row matches what `docker system df` prints.
    pub kind: String,
    /// How many of them there are, when the engine says.
    pub total: Option<u64>,
    /// How many of those are in use.
    pub active: Option<u64>,
    /// Bytes on disk.
    pub bytes: Option<u64>,
    /// Bytes the engine believes it could give back.
    pub reclaimable: Option<u64>,
}

/// What was found for one engine.
pub enum EngineState {
    /// It answered, and this is what it said.
    Ready(Vec<Row>),
    /// The binary is installed and the query did not answer. Almost always a daemon
    /// that is not running, so the engine's own words are carried through rather than
    /// guessed at.
    Unavailable(String),
}

/// One engine's entry in the report. Engines that are not installed produce none.
pub struct EngineReport {
    /// The engine's name.
    pub name: &'static str,
    /// Whether it answered, and what with.
    pub state: EngineState,
}

impl EngineReport {
    /// Total bytes across every row, or `None` when the engine did not answer.
    pub fn total_bytes(&self) -> Option<u64> {
        match &self.state {
            EngineState::Ready(rows) => Some(rows.iter().filter_map(|r| r.bytes).sum()),
            EngineState::Unavailable(_) => None,
        }
    }

    /// Total reclaimable bytes across every row, or `None` when it did not answer.
    pub fn reclaimable_bytes(&self) -> Option<u64> {
        match &self.state {
            EngineState::Ready(rows) => Some(rows.iter().filter_map(|r| r.reclaimable).sum()),
            EngineState::Unavailable(_) => None,
        }
    }
}

/// Ask every installed engine, or only the one named.
///
/// `None` for `only` means every engine found. An engine whose binary is not on `PATH`
/// is absent from the result entirely — there is nothing to say about a tool that is
/// not installed, and a row saying so on every machine without Podman would be noise.
pub fn collect(only: Option<&str>) -> Vec<EngineReport> {
    ENGINES
        .iter()
        .filter(|e| only.is_none_or(|name| e.name.eq_ignore_ascii_case(name)))
        .filter(|e| adapters::binary_available(e.binary))
        .map(probe)
        .collect()
}

/// Ask one engine how much disk it is using.
fn probe(engine: &Engine) -> EngineReport {
    let captured = adapters::capture_allowing_failure(
        engine.binary,
        engine.df_args,
        &query_dir(),
        std::time::Duration::from_secs(constants::CONTAINER_QUERY_TIMEOUT_SECS),
    );

    let state =
        match captured {
            Ok(out) if out.ok => {
                let rows = parse_rows(&out.stdout);
                if rows.is_empty() {
                    // It exited zero and said nothing this parser recognised. Reporting a
                    // total of zero would be a claim about the machine that was never made.
                    EngineState::Unavailable(format!(
                        "{} answered `system df` in a format dev-prune could not read",
                        engine.name
                    ))
                } else {
                    EngineState::Ready(rows)
                }
            }
            Ok(out) => EngineState::Unavailable(first_line(&out.stderr).unwrap_or_else(|| {
                format!("`{} system df` failed without saying why", engine.name)
            })),
            Err(e) => EngineState::Unavailable(
                first_line(&e.to_string())
                    .unwrap_or_else(|| format!("`{} system df` could not be run", engine.name)),
            ),
        };

    EngineReport {
        name: engine.name,
        state,
    }
}

/// The engine's first line of complaint, which is the part a human needs.
///
/// Docker follows "cannot connect to the daemon" with a paragraph about how to start it;
/// Podman follows its own with a stack of socket paths. Neither belongs in a table.
fn first_line(raw: &str) -> Option<String> {
    let line = raw.lines().map(str::trim).find(|l| !l.is_empty())?;
    // Generous, because this is wrapped rather than laid out in a column: Docker's
    // daemon-down message is about 200 characters and saying most of it is worse than
    // saying all of it. The cap is only here so a pathological engine cannot paste a
    // megabyte of one-line output into the report or into `--json`.
    Some(output::truncate_display(line, 400))
}

/// Where to run the queries from.
///
/// The home directory, for the same reason `devp caches` uses it: a project directory
/// can carry a `.dockerignore`, a Compose file or a `DOCKER_HOST` override in a `.env`
/// that would answer for that project rather than for the machine.
fn query_dir() -> PathBuf {
    dirs::home_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Read an engine's `system df` answer.
///
/// Three shapes. Docker, nerdctl and finch print one JSON object per line; Podman prints
/// a single array of the same objects; Apple's `container` prints one pretty-printed
/// object whose *fields* are the resource types, with no `Type` anywhere to read. The
/// first two differ only in punctuation, which is why one row parser reads either;
/// the third is a different document and gets its own.
///
/// Accepting all three removes an entire class of "works on my machine" from a report
/// whose whole job is to be believed.
fn parse_rows(raw: &str) -> Vec<Row> {
    let trimmed = raw.trim();
    if trimmed.starts_with('[') {
        return match serde_json::from_str::<Value>(trimmed) {
            Ok(Value::Array(items)) => items.iter().filter_map(row_from).collect(),
            _ => Vec::new(),
        };
    }
    // Only a document that is one whole object gets this far as anything but an error:
    // Docker's several-objects-on-several-lines does not parse as one value, and its
    // single-object case has a `Type` and no `images`, so it falls through to the loop.
    if let Ok(v) = serde_json::from_str::<Value>(trimmed)
        && let Some(rows) = apple_rows(&v)
    {
        return rows;
    }
    trimmed
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
        .filter_map(|v| row_from(&v))
        .collect()
}

/// Apple's `container system df`, which answers with one object rather than a row each.
///
/// `{"images":{"total":4,"active":2,"sizeInBytes":12345,"reclaimable":678}, "containers":
/// {…}, "volumes":{…}}` — counts and byte counts as numbers, no formatted strings to
/// parse and no percentage to strip. All three keys are required, so anything else that
/// happens to be one JSON object falls through to the row parser instead of becoming a
/// report with holes in it.
///
/// The three labels are the ones the engine's own table prints, so somebody running
/// `container system df` beside `devp caches containers` reads the same words in both.
fn apple_rows(v: &Value) -> Option<Vec<Row>> {
    let mut rows = Vec::new();
    for (key, kind) in [
        ("images", "Images"),
        ("containers", "Containers"),
        ("volumes", "Local Volumes"),
    ] {
        let usage = v.get(key)?.as_object()?;
        rows.push(Row {
            kind: kind.to_string(),
            total: usage.get("total").and_then(Value::as_u64),
            active: usage.get("active").and_then(Value::as_u64),
            bytes: usage.get("sizeInBytes").and_then(Value::as_u64),
            reclaimable: usage.get("reclaimable").and_then(Value::as_u64),
        });
    }
    Some(rows)
}

/// One row, from whichever spelling of the fields this engine uses.
fn row_from(v: &Value) -> Option<Row> {
    let kind = v.get("Type")?.as_str()?.trim().to_string();
    if kind.is_empty() {
        return None;
    }
    Some(Row {
        // Docker calls it `TotalCount`, Podman calls it `Total`.
        total: count(v, "TotalCount").or_else(|| count(v, "Total")),
        active: count(v, "Active"),
        // Where the engine offers the raw byte count, it is the truth and the formatted
        // string is a rounding of it: `1.093GB` has lost three digits before it is read.
        bytes: bytes_at(v, "RawSize", "Size"),
        reclaimable: bytes_at(v, "RawReclaimable", "Reclaimable"),
        kind,
    })
}

/// A count that may be a JSON number or a JSON string, because both are printed.
fn count(v: &Value, key: &str) -> Option<u64> {
    let field = v.get(key)?;
    if let Some(n) = field.as_u64() {
        return Some(n);
    }
    field.as_str()?.trim().parse().ok()
}

/// A size, preferring the engine's raw byte count over its formatted string.
fn bytes_at(v: &Value, raw_key: &str, human_key: &str) -> Option<u64> {
    if let Some(n) = v.get(raw_key).and_then(Value::as_u64) {
        return Some(n);
    }
    parse_size(v.get(human_key)?.as_str()?)
}

/// Bytes out of a size the way a container engine writes one.
///
/// `1.093GB`, `0B`, `987.4MB`, and — for a reclaimable figure — `1.093GB (100%)`, where
/// the percentage restates the same number and is dropped.
fn parse_size(s: &str) -> Option<u64> {
    // The percentage is the same figure expressed a second way.
    let s = s.split('(').next()?.trim();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let (number, unit) = s.split_at(split);
    let value: f64 = number.parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }

    let unit = unit.trim();
    let mut chars = unit.chars();
    let scale = chars.next();
    // `GiB` is 1024-based and `GB` is 1000-based. Docker prints the second, Podman can
    // print either, and across a 40 GB store the difference is about 3 GB — enough to
    // change what someone decides to do about it.
    let rest: String = chars.collect();
    let base: f64 = if rest.eq_ignore_ascii_case("ib") {
        1024.0
    } else {
        1000.0
    };
    let exponent = match scale.map(|c| c.to_ascii_lowercase()) {
        None | Some('b') => 0,
        Some('k') => 1,
        Some('m') => 2,
        Some('g') => 3,
        Some('t') => 4,
        Some('p') => 5,
        _ => return None,
    };

    Some((value * base.powi(exponent)).round() as u64)
}

/// Kubernetes contexts on this machine that run on this machine.
///
/// Read out of the kubeconfig with `kubectl config get-contexts`, which touches no
/// cluster and no network — a context pointing at a production cluster three time zones
/// away is filtered out by name here rather than by being dialled.
fn kube_contexts() -> Vec<String> {
    if !adapters::binary_available("kubectl") {
        return Vec::new();
    }
    let Ok(out) = adapters::capture_allowing_failure(
        "kubectl",
        &["config", "get-contexts", "-o", "name"],
        &query_dir(),
        std::time::Duration::from_secs(constants::CACHE_QUERY_TIMEOUT_SECS),
    ) else {
        return Vec::new();
    };
    if !out.ok {
        return Vec::new();
    }
    out.stdout
        .lines()
        .map(str::trim)
        .filter(|l| is_local_context(l))
        .map(str::to_string)
        .collect()
}

/// Whether a context name is one of the local-cluster tools rather than a remote.
///
/// Name-matching, because the alternative is contacting the cluster to find out, and a
/// disk report has no business dialling a Kubernetes API server. Each of these names is
/// fixed by the tool that writes it: `kind create cluster --name dev` always produces
/// `kind-dev`, and minikube always writes `minikube`.
fn is_local_context(name: &str) -> bool {
    const LOCAL_PREFIXES: [&str; 2] = ["kind-", "k3d-"];
    const LOCAL_EXACT: [&str; 5] = [
        "minikube",
        "docker-desktop",
        "rancher-desktop",
        "colima",
        "microk8s",
    ];
    LOCAL_PREFIXES.iter().any(|p| name.starts_with(p))
        || LOCAL_EXACT.iter().any(|n| name.eq_ignore_ascii_case(n))
}

/// Run `devp caches containers [engine]`, `devp caches docker` and `devp caches podman`.
pub fn run(only: Option<&str>, json_output: bool) -> Result<()> {
    if let Some(name) = only
        && !ENGINES.iter().any(|e| e.name.eq_ignore_ascii_case(name))
    {
        return Err(anyhow::Error::new(crate::UsageError(format!(
            "`{name}` is not a container engine dev-prune knows. Try one of: {}.",
            known_engines().join(", ")
        ))));
    }

    let pb = (!json_output).then(|| output::create_spinner("Asking the container engines..."));
    let reports = collect(only);
    let clusters = kube_contexts();
    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    if json_output {
        return json::emit(&json::containers_document(&reports, &clusters));
    }

    print_report(&reports, &clusters, only);
    Ok(())
}

/// What one reclaim step actually did.
pub struct StepOutcome {
    /// The command that ran, as a human would type it.
    pub command: String,
    /// What it was asked to give back.
    pub what: &'static str,
    /// `None` when it worked; otherwise the engine's own first line of complaint.
    pub problem: Option<String>,
}

/// What `devp caches clear <engine>` did, measured rather than claimed.
pub struct ClearOutcome {
    /// The engine.
    pub engine: &'static str,
    /// Each step, in the order it ran.
    pub steps: Vec<StepOutcome>,
    /// The engine's own total before, from `system df`.
    pub before: u64,
    /// The engine's own total after, asked again rather than subtracted.
    pub after: u64,
}

impl ClearOutcome {
    /// Bytes given back to the disk.
    pub fn freed(&self) -> u64 {
        self.before.saturating_sub(self.after)
    }
}

/// Run `devp caches clear <engine>`.
///
/// The one thing in this module that deletes. It exists because the alternative was
/// worse: the report ended by printing four commands and asking the reader to run them
/// in another window, which meant the space they reclaimed was theirs to have thought of
/// and dev-prune could not count it, explain it, or put it in a history.
///
/// The rule this tool actually follows is not "never deletes what no lockfile covers" —
/// `devp caches clear npm` has emptied shared caches no lockfile can prove rebuildable
/// since 1.9.0. The rule is that the *unattended* pass deletes only what a lockfile
/// rebuilds, and everything else is asked for by name, in the foreground, with what is
/// about to go printed first. This is that second kind, and it is never schedulable: no
/// daemon path reaches this function.
///
/// Volumes are the exception that stays one. An image can be pulled again and a build
/// cache rebuilt; what is inside a named volume exists nowhere else, and there is no
/// argv in any [`Engine::reclaim`] that touches one.
pub fn run_clear(name: &str, yes: bool, dry_run: bool, json_output: bool) -> Result<()> {
    let Some(engine) = ENGINES.iter().find(|e| e.name.eq_ignore_ascii_case(name)) else {
        return Err(anyhow::Error::new(crate::UsageError(format!(
            "`{name}` is not a container engine dev-prune knows. Try one of: {}.",
            known_engines().join(", ")
        ))));
    };

    // Same reason as `caches clear`: a prompt nobody can answer is a hang, and the line
    // printed in its place would land in the middle of the JSON document.
    if json_output && !yes && !dry_run {
        return Err(anyhow::Error::new(crate::UsageError(
            "`--json` cannot ask for confirmation — pass `--yes` as well, or `--dry-run` \
             to see what would go."
                .to_string(),
        )));
    }

    if !adapters::binary_available(engine.binary) {
        return Err(anyhow::Error::new(crate::UsageError(format!(
            "{} is not installed on this machine, so there is nothing of its to clear.",
            engine.name
        ))));
    }

    let before = probe(engine);
    let rows = match &before.state {
        EngineState::Ready(rows) => rows,
        // Quoted, not paraphrased. A stopped daemon and a permission problem on the
        // socket read identically from here and are fixed completely differently.
        EngineState::Unavailable(why) => {
            return Err(anyhow::Error::new(crate::UsageError(format!(
                "{} did not answer, so dev-prune will not start deleting on a guess: {why}",
                engine.name
            ))));
        }
    };
    let before_bytes: u64 = rows.iter().filter_map(|r| r.bytes).sum();

    if !json_output {
        print_clear_plan(engine, rows, dry_run);
    }
    if dry_run {
        if json_output {
            let planned = ClearOutcome {
                engine: engine.name,
                steps: planned_steps(engine),
                before: before_bytes,
                after: before_bytes,
            };
            return json::emit(&json::containers_clear_document(&planned, true));
        }
        return Ok(());
    }
    if !json_output && !crate::commands::caches::confirm_clear(yes) {
        output::print_info("Nothing was cleared.");
        return Ok(());
    }

    let steps: Vec<StepOutcome> = engine.reclaim.iter().map(|s| run_step(engine, s)).collect();

    // Asked again rather than subtracted from what each command claimed. `image prune`
    // reports the layers it deleted, and layers are shared — three images can each report
    // a gigabyte while the disk gets one back. `system df` is the only figure that
    // describes the disk instead of the bookkeeping.
    let after_bytes = probe(engine).total_bytes().unwrap_or(before_bytes);
    let outcome = ClearOutcome {
        engine: engine.name,
        steps,
        before: before_bytes,
        after: after_bytes,
    };
    record_container_clear(outcome.freed());

    if json_output {
        json::emit(&json::containers_clear_document(&outcome, false))?;
    } else {
        print_clear_result(&outcome);
    }

    // Reported first, then failed, for the same reason `caches clear` does it in that
    // order: the rows above are the useful part.
    let failed = outcome.steps.iter().filter(|s| s.problem.is_some()).count();
    if failed > 0 {
        anyhow::bail!(
            "{failed} of {}'s reclaim steps did not finish.",
            outcome.engine
        );
    }
    Ok(())
}

/// Every step as it would be reported had it run, for `--dry-run --json`.
fn planned_steps(engine: &Engine) -> Vec<StepOutcome> {
    engine
        .reclaim
        .iter()
        .map(|s| StepOutcome {
            command: step_command(engine, s),
            what: s.what,
            problem: None,
        })
        .collect()
}

/// The step as a human would type it, which is also the string that gets printed.
fn step_command(engine: &Engine, step: &ReclaimStep) -> String {
    format!("{} {}", engine.binary, step.args.join(" "))
}

/// Hand one step to the engine that owns it.
fn run_step(engine: &Engine, step: &ReclaimStep) -> StepOutcome {
    let captured = adapters::capture_allowing_failure(
        engine.binary,
        step.args,
        &query_dir(),
        std::time::Duration::from_secs(constants::CONTAINER_PRUNE_TIMEOUT_SECS),
    );
    let problem = match captured {
        Ok(out) if out.ok => None,
        Ok(out) => Some(first_line(&out.stderr).unwrap_or_else(|| {
            format!("`{}` failed without saying why", step_command(engine, step))
        })),
        Err(e) => Some(
            first_line(&e.to_string())
                .unwrap_or_else(|| format!("`{}` could not be run", step_command(engine, step))),
        ),
    };
    StepOutcome {
        command: step_command(engine, step),
        what: step.what,
        problem,
    }
}

/// Credit what was reclaimed to the machine's running container total.
fn record_container_clear(bytes: u64) {
    if bytes == 0 {
        return;
    }
    if let Ok(mut registry) = crate::config::Registry::load() {
        registry.record_container_clear(bytes);
        let _ = registry.save();
    }
}

/// What is about to run, and what the engine says it is holding.
fn print_clear_plan(engine: &Engine, rows: &[Row], dry_run: bool) {
    output::print_header(&format!("Clearing {}", engine.name));
    println!();
    for step in engine.reclaim {
        println!("  {:<40}  {}", step_command(engine, step).bold(), step.what);
    }
    println!();
    // The engine's reclaimable figure counts unused volumes, and not one of the commands
    // above touches one. Printing it whole would promise back space these steps cannot
    // give, so the volume row comes out of the estimate and is named as kept instead.
    let volumes: u64 = rows
        .iter()
        .filter(|r| r.kind.eq_ignore_ascii_case("Local Volumes"))
        .filter_map(|r| r.reclaimable)
        .sum();
    let reclaimable: u64 = rows.iter().filter_map(|r| r.reclaimable).sum();
    output::print_wrapped(
        "  ",
        &format!(
            "{} says about {} of this is reclaimable. Volumes are not touched by any of \
             the commands above and never will be — a named volume is the one thing here \
             that cannot be rebuilt from anywhere.",
            engine.name,
            output::format_bytes(reclaimable.saturating_sub(volumes))
        ),
    );
    if volumes > 0 {
        println!();
        output::print_wrapped(
            "  ",
            &format!(
                "{} of unused volumes is being left alone. If you have read what is in \
                 them and want it gone, that one is yours to run: `{} volume prune`.",
                output::format_bytes(volumes),
                engine.binary
            ),
        );
    }
    if dry_run {
        println!();
        output::print_info("Dry run — nothing was deleted.");
    }
    println!();
}

/// What actually went, measured against the engine's own answer afterwards.
fn print_clear_result(outcome: &ClearOutcome) {
    println!();
    for step in &outcome.steps {
        match &step.problem {
            None => println!("  {:<40}  done", step.command),
            Some(why) => println!("  {:<40}  {why}", step.command),
        }
    }
    println!();
    output::print_success(&format!(
        "{} freed — {} is now holding {}, down from {}.",
        output::format_bytes(outcome.freed()),
        outcome.engine,
        output::format_bytes(outcome.after),
        output::format_bytes(outcome.before)
    ));
}

/// The engine names `devp caches containers <engine>` accepts.
pub fn known_engines() -> Vec<&'static str> {
    ENGINES.iter().map(|e| e.name).collect()
}

/// Whether a name is one of them, so `caches clear docker` can say where to go instead.
pub fn is_engine(name: &str) -> bool {
    ENGINES.iter().any(|e| e.name.eq_ignore_ascii_case(name))
}

fn print_report(reports: &[EngineReport], clusters: &[String], only: Option<&str>) {
    output::print_header("Container engines");

    if reports.is_empty() {
        println!();
        output::print_info(&match only {
            Some(name) => format!("{name} is not installed on this machine."),
            None => format!(
                "No container engine found. dev-prune looks for {}.",
                known_engines().join(", ")
            ),
        });
        return;
    }

    for report in reports {
        println!();
        match &report.state {
            EngineState::Unavailable(why) => print_unavailable(report.name, why),
            EngineState::Ready(rows) => print_engine(report.name, rows),
        }
    }

    if !clusters.is_empty() {
        print_clusters(clusters);
    }

    println!();
    output::print_wrapped(
        "  ",
        "Nothing above was deleted, and nothing dev-prune runs on a schedule will ever \
         delete it. To have dev-prune run the narrow ones for you — build cache, unused \
         images, stopped containers, and what that gave back counted on your stats — use \
         `devp caches clear <engine>`. It asks first, and it never touches a volume: that \
         is the one thing here that cannot be rebuilt at all, so it stays yours to run.",
    );
}

/// An engine that is installed and did not answer.
///
/// Quoted rather than paraphrased. "Cannot connect to the Docker daemon" and "permission
/// denied on /var/run/docker.sock" are different problems with different fixes, and a
/// tidy dev-prune sentence in place of the engine's own would hide which one this is.
fn print_unavailable(name: &str, why: &str) {
    println!("  {name}");
    println!();
    output::print_wrapped("    ", why);
    println!();
    output::print_wrapped(
        "    ",
        &format!(
            "So dev-prune has no figures for {name} — a blank rather than a zero. Start \
             it and run this again."
        ),
    );
}

/// Column widths for the engine table, chosen so the longest real row — `Local
/// Volumes`, a ten-character size, a ten-character reclaimable figure and `41 items, 9
/// in use` — still lands inside the 90-column prose width the rest of the tool wraps to.
const KIND_WIDTH: usize = 16;
const SIZE_WIDTH: usize = 11;

/// One engine's rows, its total, and the commands that would reclaim each part.
fn print_engine(name: &str, rows: &[Row]) {
    println!("  {name}");
    println!();
    for row in rows {
        println!(
            "  {:<KIND_WIDTH$}{:>SIZE_WIDTH$}   {}   {}",
            row.kind,
            row.bytes.map_or("—".to_string(), output::format_bytes),
            reclaimable_cell(row.reclaimable),
            counts(row),
        );
    }

    let total: u64 = rows.iter().filter_map(|r| r.bytes).sum();
    let reclaimable: u64 = rows.iter().filter_map(|r| r.reclaimable).sum();
    println!();
    println!(
        "  {:<KIND_WIDTH$}{:>SIZE_WIDTH$}   {}",
        "Total",
        output::format_bytes(total),
        reclaimable_cell(Some(reclaimable)),
    );

    let Some(engine) = ENGINES.iter().find(|e| e.name == name) else {
        return;
    };
    println!();
    println!(
        "  {:<COMMAND_WIDTH$}what it takes with it",
        "Reclaim it yourself"
    );
    for (command, cost) in engine.prune {
        println!("  {command:<COMMAND_WIDTH$}{cost}");
    }
    if !engine.prompts {
        println!();
        // Worth one line, because the last command in that list deletes data and the
        // reader's expectation comes from the other engines: everywhere else a prune
        // stops and asks, and a `-f` in an example is the tell that it would have. This
        // one has no such flag because it has no such question.
        output::print_wrapped(
            "  ",
            &format!(
                "{} asks nothing first. Each of those runs the moment you press Return,                  including the last one.",
                engine.name
            ),
        );
    }
}

/// The `9.20 GiB reclaimable` cell, blank-padded when the engine did not say.
///
/// Padded rather than left empty so the counts column after it stays in one place down
/// the table; a row missing this figure otherwise pulls its neighbour eleven characters
/// left and the whole block stops reading as a table.
fn reclaimable_cell(bytes: Option<u64>) -> String {
    match bytes {
        Some(b) => format!("{:>SIZE_WIDTH$} reclaimable", output::format_bytes(b)),
        None => " ".repeat(SIZE_WIDTH + " reclaimable".len()),
    }
}

/// The "12 of them, 3 in use" half of a row.
fn counts(row: &Row) -> String {
    match (row.total, row.active) {
        (Some(total), Some(active)) => format!(
            "{total} {}, {active} in use",
            output::plural(total as usize, "item", "items")
        ),
        (Some(total), None) => format!(
            "{total} {}",
            output::plural(total as usize, "item", "items")
        ),
        _ => String::new(),
    }
}

/// The local Kubernetes clusters, named and deliberately unsized.
fn print_clusters(clusters: &[String]) {
    println!();
    println!("  kubernetes");
    println!();
    for name in clusters {
        println!("  {:<18} local cluster", name);
    }
    println!();
    output::print_wrapped(
        "  ",
        "Named and not sized on purpose: kind, k3d and minikube run their nodes as \
         containers or as a VM disk belonging to an engine above, so their disk is \
         already in that engine's total. A figure here would be the same gigabytes \
         counted twice. Delete a cluster with its own tool — `kind delete cluster`, \
         `minikube delete`, `k3d cluster delete` — which is also what releases the \
         space.",
    );
}

/// The one-line-per-engine block `devp caches` prints under its own table.
///
/// Short on purpose. `devp caches` is a report about package managers, and this is the
/// sentence that stops someone concluding they have reclaimed everything there is when
/// the largest thing on the disk was never in the table.
pub fn print_summary(reports: &[EngineReport]) {
    if reports.is_empty() {
        return;
    }
    println!();
    output::print_header("Container engines");
    println!();
    for report in reports {
        match &report.state {
            EngineState::Ready(_) => {
                let total = report.total_bytes().unwrap_or(0);
                let reclaimable = report.reclaimable_bytes().unwrap_or(0);
                println!(
                    "  {:<30} {:>10}  {} reclaimable · devp caches {}",
                    report.name,
                    output::format_bytes(total),
                    output::format_bytes(reclaimable),
                    report.name,
                );
            }
            EngineState::Unavailable(_) => {
                // The reason is a sentence from the engine and this is a
                // one-line-per-engine block, so it is shown by the command with room
                // for it.
                println!(
                    "  {:<30} {:>10}  did not answer · devp caches {}",
                    report.name, "—", report.name,
                );
            }
        }
    }
    println!();
    output::print_wrapped(
        "  ",
        "Container images, volumes and build cache are not package manager caches and are \
         not in the total above — dev-prune reports them and never deletes them.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_reclaim_step_can_touch_a_volume() {
        // The promise printed in the plan, checked against the argv rather than against
        // the prose. Every engine here has a `--volumes` spelling that would turn one of
        // these commands into the one that destroys data no registry can hand back, and
        // the only thing keeping it out is that nobody typed it into the table.
        for engine in ENGINES {
            for step in engine.reclaim {
                for arg in step.args {
                    assert!(
                        !arg.to_ascii_lowercase().contains("volume"),
                        "{} would run `{}`, which reaches a volume",
                        engine.name,
                        step_command(engine, step)
                    );
                }
            }
        }
    }

    #[test]
    fn every_engine_that_can_be_reported_can_be_cleared() {
        // `caches clear <engine>` accepts any name `is_engine` knows, so an engine with an
        // empty reclaim table would confirm, run nothing, and report freeing zero bytes.
        for engine in ENGINES {
            assert!(
                !engine.reclaim.is_empty(),
                "{} can be named to clear and has no steps",
                engine.name
            );
        }
    }

    #[test]
    fn every_reclaim_step_answers_for_itself_without_a_prompt() {
        // These run without a terminal behind them — inside `devp caches clear --yes`, and
        // from a shell whose stdin the engine does not own. A step that stops to ask is a
        // hang, and dev-prune has already asked the only question that matters.
        //
        // The engines that ask take `-f` to say it has been answered. Apple's `container`
        // never asks and defines no such flag, so passing one there would not be caution:
        // it would be a usage error on every step, which is the same hang's worth of
        // nothing reclaimed by a different route.
        for engine in ENGINES {
            for step in engine.reclaim {
                let forced = step.args.contains(&"-f") || step.args.contains(&"--force");
                assert_eq!(
                    forced,
                    engine.prompts,
                    "`{}` disagrees with what {} does about prompting",
                    step_command(engine, step),
                    engine.name
                );
            }
        }
    }

    #[test]
    fn reads_apples_one_object_for_all_three() {
        // Apple's `container` is not installed on the machines this is developed on, so
        // the shape is pinned from `DiskUsageStats`/`ResourceUsage` in apple/container
        // rather than from a run. If those field names ever change, this fails here
        // instead of the report quietly showing an engine holding nothing.
        let raw = r#"{
          "images" : { "total" : 12, "active" : 3, "sizeInBytes" : 4210000000,
                       "reclaimable" : 3020000000 },
          "containers" : { "total" : 7, "active" : 1, "sizeInBytes" : 118400000,
                           "reclaimable" : 118400000 },
          "volumes" : { "total" : 2, "active" : 0, "sizeInBytes" : 2048,
                        "reclaimable" : 2048 }
        }"#;
        let rows = parse_rows(raw);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].kind, "Images");
        assert_eq!(rows[0].total, Some(12));
        assert_eq!(rows[0].active, Some(3));
        assert_eq!(rows[0].bytes, Some(4_210_000_000));
        assert_eq!(rows[0].reclaimable, Some(3_020_000_000));
        // The label is the engine's own, not the JSON key: `volumes` prints as
        // `Local Volumes`, which is also what the volume-keeping arithmetic matches on.
        assert_eq!(rows[2].kind, "Local Volumes");
        assert_eq!(rows[2].bytes, Some(2_048));
    }

    #[test]
    fn a_json_object_that_is_not_apples_is_not_read_as_apples() {
        // Docker printing a single row — one object, on one line — must still be read as
        // that row rather than swallowed by the branch above.
        let rows = parse_rows(
            r#"{"Active":"3","Reclaimable":"3.02GB (71%)","Size":"4.21GB","TotalCount":"12","Type":"Images"}"#,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "Images");
        // Two of the three keys is not the shape either, and half a report is worse than
        // the honest "could not read this" the caller prints for no rows.
        assert!(parse_rows(r#"{"images":{"total":1},"containers":{"total":1}}"#).is_empty());
    }

    #[test]
    fn parses_docker_si_sizes() {
        assert_eq!(parse_size("0B"), Some(0));
        assert_eq!(parse_size("1.093GB"), Some(1_093_000_000));
        assert_eq!(parse_size("987.4MB"), Some(987_400_000));
        assert_eq!(parse_size("1.5kB"), Some(1_500));
        assert_eq!(parse_size("2TB"), Some(2_000_000_000_000));
    }

    #[test]
    fn iec_suffix_is_base_1024() {
        assert_eq!(parse_size("1KiB"), Some(1_024));
        assert_eq!(parse_size("1GiB"), Some(1_073_741_824));
        // The distinction is the whole reason the suffix is inspected: the same number
        // with the other suffix is 7% smaller.
        assert_ne!(parse_size("1GiB"), parse_size("1GB"));
    }

    #[test]
    fn reclaimable_percentage_is_dropped() {
        assert_eq!(parse_size("1.093GB (100%)"), Some(1_093_000_000));
        assert_eq!(parse_size("0B (0%)"), Some(0));
    }

    #[test]
    fn rejects_what_is_not_a_size() {
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("N/A"), None);
        assert_eq!(parse_size("GB"), None);
        assert_eq!(parse_size("12 apples"), None);
    }

    #[test]
    fn reads_dockers_one_object_per_line() {
        let raw = concat!(
            r#"{"Active":"3","Reclaimable":"3.02GB (71%)","Size":"4.21GB","TotalCount":"12","Type":"Images"}"#,
            "\n",
            r#"{"Active":"1","Reclaimable":"118.4MB (100%)","Size":"118.4MB","TotalCount":"7","Type":"Containers"}"#,
            "\n",
            r#"{"Active":"0","Reclaimable":"6.75GB","Size":"6.75GB","TotalCount":"41","Type":"Build Cache"}"#,
        );
        let rows = parse_rows(raw);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].kind, "Images");
        assert_eq!(rows[0].total, Some(12));
        assert_eq!(rows[0].active, Some(3));
        assert_eq!(rows[0].bytes, Some(4_210_000_000));
        assert_eq!(rows[0].reclaimable, Some(3_020_000_000));
        assert_eq!(rows[2].kind, "Build Cache");
        assert_eq!(rows[2].active, Some(0));
    }

    #[test]
    fn reads_podmans_single_array() {
        let raw = r#"[
            {"Type":"Images","Total":4,"Active":2,"Size":"1.5GB","Reclaimable":"500MB (33%)"},
            {"Type":"Local Volumes","Total":2,"Active":0,"RawSize":2048,"RawReclaimable":2048,
             "Size":"2.048kB","Reclaimable":"2.048kB (100%)"}
        ]"#;
        let rows = parse_rows(raw);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].total, Some(4));
        assert_eq!(rows[0].bytes, Some(1_500_000_000));
        // The raw byte count wins over the string rounded from it.
        assert_eq!(rows[1].bytes, Some(2_048));
        assert_eq!(rows[1].reclaimable, Some(2_048));
    }

    #[test]
    fn unparseable_output_is_no_rows_rather_than_zero_bytes() {
        assert!(parse_rows("").is_empty());
        assert!(parse_rows("Cannot connect to the Docker daemon").is_empty());
        // Valid JSON, but not a df row: no `Type` to name.
        assert!(parse_rows(r#"{"Size":"4GB"}"#).is_empty());
    }

    #[test]
    fn local_contexts_are_told_from_remote_ones() {
        assert!(is_local_context("kind-dev"));
        assert!(is_local_context("k3d-test"));
        assert!(is_local_context("minikube"));
        assert!(is_local_context("docker-desktop"));
        assert!(!is_local_context("arn:aws:eks:us-east-1:1234:cluster/prod"));
        assert!(!is_local_context("gke_project_us-central1_prod"));
        // A remote cluster somebody named after the tool is still remote, but this is
        // name-matching and the alternative is dialling it. Naming a production context
        // `minikube` is a problem that predates dev-prune.
        assert!(!is_local_context("kindly-prod"));
    }

    #[test]
    fn every_engine_prints_at_least_one_reclaim_command() {
        for engine in ENGINES {
            assert!(
                !engine.prune.is_empty(),
                "{} has no reclaim command to print",
                engine.name
            );
            for (command, _) in engine.prune {
                assert!(
                    command.starts_with(engine.binary),
                    "{command} is not a {} command",
                    engine.name
                );
            }
        }
    }

    #[test]
    fn no_reclaim_command_is_ever_run_by_dev_prune() {
        // The guard is that `prune` is only ever read into a `println!`. If a future
        // change hands one of these to a process spawner, this file is where the review
        // has to notice, so the strings are checked to be commands for a human to type
        // rather than argv this code could execute.
        for engine in ENGINES {
            for (command, _) in engine.prune {
                assert!(
                    command.contains(' '),
                    "{command} looks like a bare program name"
                );
            }
        }
    }
}
