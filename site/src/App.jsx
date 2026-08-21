// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

import React, { useState, useEffect, useRef } from "react";
import {
  Check,
  Copy,
  Cpu,
  GitBranch,
  HardDrive,
  ExternalLink,
  Sparkles,
  Lock,
  FileCode,
  ShieldCheck,
  Bot,
  Layers,
  Link2,
  Terminal,
  Gauge,
  EyeOff,
  Undo2,
  ChevronDown,
  Puzzle,
} from "lucide-react";

const REPO = "https://github.com/Life-Experimentalist/dev-prune";
const DOCS = `${REPO}/blob/main/docs`;
const VERSION = "1.2.1";
const THEME_KEY = "devprune-theme";

/* ------------------------------------------------------------------ */
/* Theme control: light, dark, or whatever the browser is set to.       */
/*                                                                      */
/* The markup is identical on the server and the client — every visual  */
/* state is driven by `data-theme` / `data-theme-choice` on <html>,     */
/* which the inline script in index.html has already set before the     */
/* first paint. React owns the click handlers and the ARIA state only,  */
/* so hydration has nothing to reconcile and there is no flash.         */
/* ------------------------------------------------------------------ */
function ThemeToggle() {
  const switchRef = useRef(null);
  const autoRef = useRef(null);

  const apply = (choice) => {
    const root = document.documentElement;
    const prefersLight =
      !!window.matchMedia &&
      window.matchMedia("(prefers-color-scheme: light)").matches;
    const resolved =
      choice === "system" ? (prefersLight ? "light" : "dark") : choice;

    root.dataset.themeChoice = choice;
    root.dataset.theme = resolved;

    const meta = document.querySelector('meta[name="theme-color"]');
    if (meta)
      meta.setAttribute("content", resolved === "light" ? "#ffffff" : "#0b0f17");
    if (switchRef.current)
      switchRef.current.setAttribute("aria-checked", String(resolved === "light"));
    if (autoRef.current)
      autoRef.current.setAttribute("aria-pressed", String(choice === "system"));
  };

  const choose = (choice) => {
    try {
      if (choice === "system") localStorage.removeItem(THEME_KEY);
      else localStorage.setItem(THEME_KEY, choice);
    } catch {
      // Storage blocked (private mode, embedded webview). The choice still
      // applies to this page; it just will not survive a reload.
    }
    apply(choice);
  };

  useEffect(() => {
    // Bring the ARIA state in line with whatever the head script decided.
    apply(document.documentElement.dataset.themeChoice || "system");

    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const follow = () => {
      if (document.documentElement.dataset.themeChoice === "system")
        apply("system");
    };
    mq.addEventListener("change", follow);
    return () => mq.removeEventListener("change", follow);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="theme-controls">
      <button
        type="button"
        ref={autoRef}
        className="theme-auto"
        aria-pressed="false"
        title="Follow the browser's colour scheme"
        onClick={() => choose("system")}
      >
        Auto
      </button>
      <button
        type="button"
        ref={switchRef}
        className="switch"
        role="switch"
        aria-checked="false"
        aria-label="Light theme"
        title="Switch between light and dark"
        onClick={() =>
          choose(
            document.documentElement.dataset.theme === "light"
              ? "dark"
              : "light",
          )
        }
      >
        <span aria-hidden="true" className="switch__label">
          <span aria-hidden="true" className="switch__indicator"></span>
          <span aria-hidden="true" className="switch__decoration"></span>
        </span>
      </button>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Reveal-on-scroll. Content renders visible; the animation is layered  */
/* on only when JS is present, so the prerendered HTML is never hidden. */
/* ------------------------------------------------------------------ */
function Reveal({
  children,
  className = "",
  delay = 0,
  as: Tag = "div",
  ...rest
}) {
  const ref = useRef(null);
  const [shown, setShown] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (typeof IntersectionObserver === "undefined") {
      setShown(true);
      return;
    }
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setShown(true);
          io.disconnect();
        }
      },
      { rootMargin: "0px 0px -10% 0px", threshold: 0.05 },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  return (
    <Tag
      ref={ref}
      className={`reveal ${shown ? "is-visible" : ""} ${className}`.trim()}
      style={delay ? { transitionDelay: `${delay}ms` } : undefined}
      {...rest}
    >
      {children}
    </Tag>
  );
}

/* ------------------------------------------------------------------ */
/* Adapters, grouped by the language they belong to. Managers that share  */
/* a bloat directory can only be told apart within their own language, so */
/* each group carries the rule that decides between its members.          */
/* ------------------------------------------------------------------ */
const ECOSYSTEMS = [
  {
    id: "eco-js",
    language: "JavaScript & TypeScript",
    summary:
      "Four managers, one directory. All of them install into node_modules, so at most one of them owns any given project.",
    rows: [
      {
        name: "npm",
        detect: <code>package-lock.json</code>,
        deletes: <code>node_modules</code>,
        verify: <code>npm ci --dry-run --ignore-scripts</code>,
        restore: <code>npm ci</code>,
      },
      {
        name: "pnpm",
        detect: <code>pnpm-lock.yaml</code>,
        deletes: <code>node_modules</code>,
        verify: <code>pnpm install --lockfile-only --frozen-lockfile</code>,
        restore: <code>pnpm install --frozen-lockfile</code>,
      },
      {
        name: "Yarn",
        detect: <code>yarn.lock</code>,
        deletes: <code>node_modules</code>,
        verify: (
          <>
            <code>yarn install --immutable --mode update-lockfile</code> (Berry)
          </>
        ),
        restore: <code>yarn install --immutable</code>,
      },
      {
        name: "Bun",
        detect: (
          <>
            <code>bun.lockb</code>, <code>bun.lock</code>
          </>
        ),
        deletes: <code>node_modules</code>,
        verify: (
          <code>bun install --frozen-lockfile --dry-run --ignore-scripts</code>
        ),
        restore: <code>bun install --frozen-lockfile</code>,
      },
    ],
    tieBreak: (
      <>
        <strong>When more than one claims the same tree,</strong> the owner is
        decided in this order: the <code>packageManager</code> field in{" "}
        <code>package.json</code>; else whichever manager's bookkeeping files
        are actually inside the installed <code>node_modules</code>; else the
        most recently written lockfile. Only that manager verifies, deletes and
        restores — the others are not consulted.
      </>
    ),
  },
  {
    id: "eco-python",
    language: "Python",
    summary:
      "Two managers that can describe the same project, because a uv project is still a directory with a virtual environment in it.",
    rows: [
      {
        name: "uv",
        detect: (
          <>
            <code>uv.lock</code>, <code>[tool.uv]</code>
          </>
        ),
        deletes: <code>.venv</code>,
        verify: <code>uv lock --locked</code>,
        restore: <code>uv sync</code>,
      },
      {
        name: "venv / pip",
        detect: (
          <>
            <code>requirements.txt</code> + <code>pyvenv.cfg</code>
          </>
        ),
        deletes: (
          <>
            every dir holding <code>pyvenv.cfg</code>
          </>
        ),
        verify: (
          <>
            <code>requirements.txt</code> lists ≥ 1 package
          </>
        ),
        restore: <code>python -m venv .venv &amp;&amp; pip install -r …</code>,
      },
    ],
    tieBreak: (
      <>
        <strong>uv wins over plain venv</strong> whenever <code>uv.lock</code>{" "}
        or a <code>[tool.uv]</code> table is present, because that lockfile
        rebuilds the environment exactly and a <code>requirements.txt</code>{" "}
        only approximates it. A project with no <code>uv.lock</code> falls back
        to venv, and a venv with an empty <code>requirements.txt</code> is
        refused outright — there would be nothing to reinstall from.
      </>
    ),
  },
  {
    id: "eco-compiled",
    language: "Rust & Go",
    summary:
      "One manager each, no ambiguity — and the clearest illustration of why every verification in these tables is read-only.",
    rows: [
      {
        name: "Cargo",
        detect: <code>Cargo.toml</code>,
        deletes: <code>target</code>,
        verify: <code>cargo metadata --locked</code>,
        restore: (
          <>
            next <code>cargo build</code>
          </>
        ),
      },
      {
        name: "Go",
        detect: <code>go.mod</code>,
        deletes: <code>vendor</code>,
        verify: <code>go mod download</code>,
        restore: <code>go mod vendor</code>,
      },
    ],
    tieBreak: (
      <>
        <strong>Read-only, in every ecosystem above.</strong>{" "}
        <code>cargo generate-lockfile</code> and <code>go mod tidy</code> make
        it obvious why: they rewrite <code>Cargo.lock</code> and{" "}
        <code>go.mod</code>/<code>go.sum</code>, tracked files that would turn a
        cleanup into a diff you did not ask for. The same is true of{" "}
        <code>npm install --package-lock-only</code> and <code>uv lock</code>,
        so none of them run during a normal pass either. A lockfile that has
        drifted from its manifest is <em>reported</em> and the directory is left
        alone — because a pass can be started by the OS scheduler, and a
        background cleanup must never leave a modified tracked file behind. If
        you would rather it fixed the lockfile for you,{" "}
        <code>devp config set allow_manifest_rewrite true</code> opts in, and it
        means the same thing for all eight adapters.
      </>
    ),
  },
];

function EcosystemGroup({ group }) {
  return (
    <div className="eco-group" id={group.id}>
      <div className="eco-group-head">
        <h3>{group.language}</h3>
        <p>{group.summary}</p>
      </div>
      <div className="table-scroll">
        <table className="data-table">
          <thead>
            <tr>
              <th>Manager</th>
              <th>Detected by</th>
              <th>Deletes</th>
              <th>
                Verified with <span className="tag-ro">read-only</span>
              </th>
              <th>Restored with</th>
            </tr>
          </thead>
          <tbody>
            {group.rows.map((row) => (
              <tr key={row.name}>
                <td className="td-name">{row.name}</td>
                <td>{row.detect}</td>
                <td>{row.deletes}</td>
                <td>{row.verify}</td>
                <td>{row.restore}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="eco-tiebreak">{group.tieBreak}</p>
    </div>
  );
}

function CopyButton({ id, text, copied, onCopy }) {
  return (
    <button
      className="copy-button"
      onClick={() => onCopy(id, text)}
      title="Copy to clipboard"
      aria-label={copied === id ? "Copied" : "Copy command to clipboard"}
    >
      {copied === id ? (
        <Check size={16} className="c-green" />
      ) : (
        <Copy size={16} />
      )}
    </button>
  );
}

function Faq({ q, children }) {
  return (
    <details className="faq-item">
      <summary>
        <span>{q}</span>
        <ChevronDown size={18} className="faq-chevron" aria-hidden="true" />
      </summary>
      <div className="faq-answer">{children}</div>
    </details>
  );
}

export default function App() {
  const [installTab, setInstallTab] = useState("bash");
  const [termTab, setTermTab] = useState("run");
  const [copiedKey, setCopiedKey] = useState(null);
  const [projectsCount, setProjectsCount] = useState(24);
  const [avgSizeGB, setAvgSizeGB] = useState(0.8);
  const [idleShare, setIdleShare] = useState(60);

  // Windows visitors get the PowerShell line first. Runs after hydration, so the
  // prerendered HTML is identical for everyone and the crawler sees a real default.
  useEffect(() => {
    const ua = (navigator.userAgent || "").toLowerCase();
    if (ua.includes("windows")) setInstallTab("powershell");
  }, []);

  const reclaimGB = (projectsCount * avgSizeGB * (idleShare / 100)).toFixed(1);

  const installCommands = {
    bash: {
      label: "Linux / macOS",
      note: "Needs a Unix shell — also fine on Windows under Git Bash, MSYS2, Cygwin or WSL. In PowerShell or Command Prompt it fails with 'sh is not recognized'; use the Windows tabs there.",
      cmd: "curl -fsSL https://devprune.vkrishna04.me/install.sh | sh",
    },
    powershell: {
      label: "Windows",
      note: "Installs to %APPDATA%\\dev-prune\\bin and registers devp for PowerShell and cmd alike.",
      cmd: "iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex",
    },
    cmdexe: {
      label: "Windows (cmd)",
      note: "Command Prompt has no iwr, so it borrows PowerShell for the download. Same install; devp resolves in the next Command Prompt you open.",
      cmd: 'powershell -NoProfile -ExecutionPolicy Bypass -Command "iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex"',
    },
    python: {
      label: "uv / pipx",
      note: "Platform wheels carrying the binary. Nothing Python runs. Swap in uvx dev-prune status to run it once and leave nothing behind, or pipx install dev-prune.",
      cmd: "uv tool install dev-prune",
    },
    pip: {
      label: "pip",
      note: "The same wheels, into whichever environment is active — a venv's Scripts/bin rather than a shared tool directory. Use pip install --user dev-prune for a machine-wide install.",
      cmd: "pip install dev-prune",
    },
    cargo: {
      label: "Cargo",
      note: "crates.io stores source, not binaries, so cargo install always compiles (Rust 1.88+). cargo binstall downloads the same prebuilt archive the installers use.",
      cmd: "cargo binstall dev-prune",
    },
    release: {
      label: "Release binary",
      note: "Every archive ships a .sha256 sidecar; the installers refuse to run without one.",
      cmd: "https://github.com/Life-Experimentalist/dev-prune/releases/latest",
    },
  };

  // The same prompt as docs/AI_SETUP_PROMPT.md, kept copy-ready right in the install
  // widget: hand it to any terminal-capable agent and it installs, verifies and registers
  // repos for you. Uses devp (never devpw) so the agent can read command output.
  const aiInstallPrompt = `Install and set up \`dev-prune\` (binary name \`devp\`), a lockfile-safe workspace cleaner, on this machine. Follow these steps exactly and do not improvise beyond them.

1. Detect the OS and run the matching official installer, nothing else:
   - macOS or Linux:
       curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
   - Windows (PowerShell):
       iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex
   - If a Rust toolchain is already present and you cannot reach the network, you may
     instead run:  cargo install dev-prune
   Do NOT download binaries from anywhere other than devprune.vkrishna04.me or the
   project's GitHub releases, and do NOT edit PATH, the registry, or any OS scheduler by
   hand — the installer and \`devp setup\` do all of that themselves.

2. Open a NEW terminal (so the updated PATH is in effect) and verify:
       devp --version
       devp doctor
   \`devp doctor\` must exit 0. If it prints warnings, read them out to me; do not try to
   "fix" the scheduler or hooks yourself — they are self-installing.

3. Ask me which project directories to keep clean, then register each one:
       devp init <path>
   Do not register directories I did not name. \`devp init\` only records a directory; it
   never deletes anything on its own.

4. Show me the result and stop:
       devp status

Notes you should rely on, not work around:
- Installation already registered a background pass (every 2 days) and, on Windows, a
  windowless task that never flashes a console window. You do not need to configure any
  of this.
- Nothing is ever deleted unless a lockfile can rebuild it, the repo has been idle past
  the threshold, and (interactively) I confirm. Run \`devp run --dry-run\` if I want a
  preview.
- To undo the whole thing later: \`devp uninstall\`.`;

  const handleCopy = (key, text) => {
    // "Copied" only after the write actually lands — an insecure context or a
    // denied permission must not report success it never had.
    if (!navigator.clipboard) return;
    navigator.clipboard.writeText(text).then(
      () => {
        setCopiedKey(key);
        setTimeout(() => setCopiedKey(null), 2000);
      },
      () => {},
    );
  };

  return (
    <div className="app">
      <a href="#main" className="skip-link">
        Skip to content
      </a>

      <div className="bg-glow bg-glow-1" aria-hidden="true"></div>
      <div className="bg-glow bg-glow-2" aria-hidden="true"></div>

      {/* ------------------------------- nav ------------------------------- */}
      <header className="navbar">
        <div className="container nav-content">
          <a href="#top" className="brand">
            <div className="logo-box">
              <img
                src="/assets/icon_small.png"
                alt=""
                width="32"
                height="32"
                className="logo-icon-img"
              />
            </div>
            <span className="brand-name">dev-prune</span>
            <span className="version-tag">v{VERSION}</span>
          </a>
          <nav className="nav-links" aria-label="Primary">
            <a href="#how">How it works</a>
            <a href="#safety">Safety</a>
            <a href="#ecosystems">Ecosystems</a>
            <a href="#commands">Commands</a>
            <a href="#ai">AI agents</a>
            <a href="#faq">FAQ</a>
            <ThemeToggle />
            <a
              href={REPO}
              target="_blank"
              rel="noreferrer"
              className="btn btn-secondary nav-github"
            >
              GitHub <ExternalLink size={14} />
            </a>
          </nav>
        </div>
      </header>

      <main id="main">
        {/* ------------------------------ hero ------------------------------ */}
        <section className="hero" id="top">
          <div className="container hero-grid">
            <div className="hero-text">
              <div className="pill-badge glow-pulse">
                <Sparkles size={14} /> v{VERSION} · Rust · Apache-2.0 · no
                analytics
              </div>
              <h1 className="hero-title">
                Get the gigabytes back.
                <br />
                <span className="gradient-text">Lose nothing.</span>
              </h1>
              <p className="hero-description">
                <strong>dev-prune</strong> deletes <code>node_modules</code>,{" "}
                <code>.venv</code>, <code>target</code> and <code>vendor</code>{" "}
                from Git repositories you have not touched in a while — but only
                after the package manager itself confirms a lockfile can rebuild
                them. Verification is not a flag you can turn off.
              </p>

              <p className="hero-alias">
                <strong>The command is <code>dev-prune</code>.</strong>{" "}
                <code>devp</code> is an alias for it — the same executable
                installed under a shorter name, purely for ease of use. Use
                either, anywhere, interchangeably; every example below works
                spelled both ways.
              </p>

              <div className="install-widget" id="install">
                <div
                  className="widget-tabs"
                  role="group"
                  aria-label="Installation method"
                >
                  {Object.entries(installCommands).map(([key, v]) => (
                    <button
                      key={key}
                      aria-pressed={installTab === key}
                      className={installTab === key ? "active" : ""}
                      onClick={() => setInstallTab(key)}
                    >
                      {v.label}
                    </button>
                  ))}
                </div>
                <div className="cmd-line">
                  <span className="cmd-prompt">$</span>
                  <code className="cmd-code">
                    {installCommands[installTab].cmd}
                  </code>
                  <CopyButton
                    id={installTab}
                    text={installCommands[installTab].cmd}
                    copied={copiedKey}
                    onCopy={handleCopy}
                  />
                </div>
                <p className="install-note">
                  {installCommands[installTab].note}
                </p>
              </div>

              <div className="ai-install" id="ai-setup">
                <div className="ai-install-head">
                  <span className="ai-install-title">
                    Or let an AI assistant do it
                  </span>
                  <CopyButton
                    id="ai-prompt"
                    text={aiInstallPrompt}
                    copied={copiedKey}
                    onCopy={handleCopy}
                  />
                </div>
                <p className="ai-install-note">
                  Copy this prompt and paste it to Claude Code, Cursor, Copilot,
                  Windsurf, or any terminal-capable agent — it installs, verifies
                  and registers your repos for you.{" "}
                  <a href="https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/AI_SETUP_PROMPT.md">
                    Details
                  </a>
                  .
                </p>
                <pre className="ai-install-prompt">{aiInstallPrompt}</pre>
              </div>

              <div className="hero-stats">
                <div>
                  <strong>8</strong>
                  <span>package managers</span>
                </div>
                <div>
                  <strong>0</strong>
                  <span>analytics or telemetry</span>
                </div>
                <div>
                  <strong>3</strong>
                  <span>platforms, one binary each</span>
                </div>
              </div>
            </div>

            {/* ---------------------------- terminal ---------------------------- */}
            <div className="terminal-container" id="terminal">
              <div className="terminal-header">
                <div className="t-dots">
                  <span className="t-dot t-red"></span>
                  <span className="t-dot t-yellow"></span>
                  <span className="t-dot t-green"></span>
                </div>
                <div
                  className="t-cmd-tabs"
                  role="group"
                  aria-label="Example command"
                >
                  {[
                    ["run", "devp run"],
                    ["status", "devp status"],
                    ["stats", "devp stats"],
                    ["restore", "devp restore"],
                    ["doctor", "devp doctor"],
                    ["setup", "devp setup --status"],
                  ].map(([key, label]) => (
                    <button
                      key={key}
                      aria-pressed={termTab === key}
                      className={termTab === key ? "active" : ""}
                      onClick={() => setTermTab(key)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="terminal-window">
                {termTab === "run" && (
                  <div className="term-body">
                    <div className="term-line">$ devp run --dry-run</div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line">
                      <span className="c-blue">→</span> Required package
                      managers:
                    </div>
                    <div className="term-line c-green">
                      {" "}
                      ✓ pnpm ✓ uv ✓ cargo
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line">
                      <span className="c-yellow">•</span> MyMonorepo →
                      frontend/node_modules (412.7 MB) [pnpm]
                    </div>
                    <div className="term-line">
                      <span className="c-yellow">•</span> MyMonorepo →
                      services/api/.venv (188.2 MB) [uv]
                    </div>
                    <div className="term-line">
                      <span className="c-yellow">•</span> MyMonorepo →
                      tools/cli/target (1.4 GB) [cargo]
                    </div>
                    <div className="term-line">
                      <span className="c-dim">•</span>{" "}
                      <span className="c-dim">
                        ActiveService — skipped (last commit 2 days ago)
                      </span>
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-bold">
                      Total reclaimable: 2.00 GB across 3 directories
                    </div>
                    <div className="term-line c-dim">
                      Dry run — nothing was deleted.
                    </div>
                  </div>
                )}

                {termTab === "status" && (
                  <div className="term-body">
                    <div className="term-line c-bold">
                      dev-prune — registered repositories
                    </div>
                    <div className="term-line c-dim">
                      ──────────────────────────────────────────────────────────
                    </div>
                    <div className="term-line c-bold">
                      {" "}
                      Repository Status Reclaimable Last activity
                    </div>
                    <div className="term-line">
                      <span className="c-green"> ▸ MyMonorepo</span> Candidate
                      2.00 GB 41 days ago
                    </div>
                    <div className="term-line">
                      {" "}
                      PyDataLab Candidate 850.0 MB 66 days ago
                    </div>
                    <div className="term-line c-dim">
                      {" "}
                      ArchivedApp Ignored 320.0 MB —
                    </div>
                    <div className="term-line c-cyan">
                      {" "}
                      ActiveService Active 3.10 GB 2 days ago
                    </div>
                    <div className="term-line c-dim">
                      ──────────────────────────────────────────────────────────
                    </div>
                    <div className="term-line c-yellow">
                      [↑/↓/j/k] Navigate [PgUp/PgDn/g/G] Jump [p] Prune [i]
                      Ignore [q] Quit
                    </div>
                    <div className="term-line c-dim">
                      dev-prune · made with ♥ by VKrishna04 ·
                      github.com/Life-Experimentalist/dev-prune
                    </div>
                  </div>
                )}

                {termTab === "stats" && (
                  <div className="term-body">
                    <div className="term-line">$ devp stats</div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-bold">Lifetime</div>
                    <div className="term-line">
                      <span className="c-blue">→</span> Space reclaimed:
                      {"   "}12.41 GB
                    </div>
                    <div className="term-line">
                      <span className="c-blue">→</span> Prune passes:
                      {"      "}9
                    </div>
                    <div className="term-line">
                      <span className="c-blue">→</span> Repositories:
                      {"      "}14 tracked
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-bold">Most recent pass</div>
                    <div className="term-line">
                      <span className="c-blue">→</span> 2026-08-11 06:00 UTC (2
                      days ago) — 2.00 GB from 3 directories
                    </div>
                    <div className="term-line">
                      <span className="c-blue">→</span> Put it back with: devp
                      restore --last-run
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-bold">Biggest reclaims</div>
                    <div className="term-line">
                      {"     "}4.20 GB   ~/Code/MyMonorepo
                      <span className="c-dim">
                        {"   "}(last pruned 2 days ago)
                      </span>
                    </div>
                    <div className="term-line">
                      {"     "}2.90 GB   ~/Code/PyDataLab
                      <span className="c-dim">
                        {"   "}(last pruned 12 days ago)
                      </span>
                    </div>
                    <div className="term-line">
                      {"    "}850.0 MB   ~/Code/ArchivedApp
                      <span className="c-dim">
                        {"   "}(last pruned 30 days ago)
                      </span>
                    </div>
                  </div>
                )}

                {termTab === "restore" && (
                  <div className="term-body">
                    <div className="term-line">
                      $ devp restore ~/Code/MyMonorepo
                    </div>
                    <div className="term-line">
                      <span className="c-blue">→</span> frontend — pnpm install
                      --frozen-lockfile
                    </div>
                    <div className="term-line c-green">
                      {" "}
                      ✓ 1,240 packages restored
                    </div>
                    <div className="term-line">
                      <span className="c-blue">→</span> services/api — uv sync
                    </div>
                    <div className="term-line c-green">
                      {" "}
                      ✓ environment recreated from uv.lock
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-green">
                      ✓ Every project in the tree is back.
                    </div>
                  </div>
                )}

                {termTab === "doctor" && (
                  <div className="term-body">
                    <div className="term-line">$ devp doctor .</div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-blue">
                      dev-prune doctor (~/Code/acme)
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-dim">Repository</div>
                    <div className="term-line">
                      {"  Git repository        "}
                      <span className="c-green">✓</span> yes
                    </div>
                    <div className="term-line">
                      {"  Registered            "}
                      <span className="c-green">✓</span> yes
                    </div>
                    <div className="term-line c-dim">
                      {"  .devprune.json          absent — global settings apply"}
                    </div>
                    <div className="term-line c-dim">
                      {"  Activity                2026-04-11 (31 days ago), threshold 15 — idle"}
                    </div>
                    <div className="term-line c-dim">
                      {"  Scan depth              6 levels below the root"}
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-dim">Projects</div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line">{"  frontend (pnpm)"}</div>
                    <div className="term-line">
                      {"    Lockfile            "}
                      <span className="c-green">✓</span> pnpm-lock.yaml present
                    </div>
                    <div className="term-line">
                      {"    Bloat               "}
                      <span className="c-green">✓</span> frontend/node_modules
                      (412.08 MiB)
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line">{"  services/api (uv)"}</div>
                    <div className="term-line">
                      {"    Lockfile            "}
                      <span className="c-red">✗</span> uv.lock missing — nothing can
                      prove
                    </div>
                    <div className="term-line">
                      {"                          the directory is rebuildable, so it will never"}
                    </div>
                    <div className="term-line">
                      {"                          be pruned"}
                    </div>
                    <div className="term-line">
                      {"    Bloat               "}
                      <span className="c-green">✓</span> services/api/.venv (218.44
                      MiB)
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-dim">Verdict</div>
                    <div className="term-line">
                      {"  "}
                      <span className="c-green">✓</span> Would `devp run` prune this?
                      Yes — frontend has verifiable bloat.
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line">
                      {"  "}
                      <span className="c-red">✗</span> services/api: uv.lock missing …
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-dim">
                      {"  Troubleshooting: .../docs/troubleshooting/README.md"}
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-red">
                      {"  Error: 1 problem found."}
                      <span className="c-dim">{"   (exit 1)"}</span>
                    </div>
                  </div>
                )}

                {termTab === "setup" && (
                  <div className="term-body">
                    <div className="term-line">$ devp setup --status</div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-green">
                      {" "}
                      ✓ devp alias matches the installed binary
                    </div>
                    <div className="term-line c-green">
                      {" "}
                      ✓ SKILL.md exported and current
                    </div>
                    <div className="term-line c-green">
                      {" "}
                      ✓ Git hooks post-commit, post-checkout, post-merge
                    </div>
                    <div className="term-line c-green">
                      {" "}
                      ✓ OS scheduler every 2 days
                    </div>
                    <div className="term-line">&nbsp;</div>
                    <div className="term-line c-dim">
                      {" "}
                      auto_setup=true auto_hooks=true auto_daemon=true
                    </div>
                    <div className="term-line c-dim"> Nothing was changed.</div>
                  </div>
                )}
              </div>
            </div>
          </div>
        </section>

        {/* ------------------------------ problem ------------------------------ */}
        <Reveal as="section" className="section" id="how">
          <div className="container">
            <div className="section-header">
              <h2 className="section-title">
                Old projects keep their dependencies{" "}
                <span className="gradient-text">forever</span>
              </h2>
              <p className="section-subtitle">
                A repository you last opened eight months ago is still holding a
                gigabyte of packages that a single command could reinstall.
                Deleting them by hand is tedious; deleting them with{" "}
                <code>rm -rf</code> and a <code>find</code> pipeline is how you
                lose a virtual environment nobody wrote a lockfile for.
              </p>
            </div>

            <div className="steps-grid">
              <div className="step-card">
                <span className="step-num">1</span>
                <h3>Register</h3>
                <p>
                  <code>devp init ~/Code</code> walks your workspace and records
                  every Git repository it finds. Git hooks then keep the list
                  current as you clone new ones.
                </p>
              </div>
              <div className="step-card">
                <span className="step-num">2</span>
                <h3>Judge idleness</h3>
                <p>
                  A repository is a candidate only after <code>idle_days</code>{" "}
                  (15 by default) with no commit <em>and</em> no source file
                  modified — so uncommitted work in progress protects itself.
                </p>
              </div>
              <div className="step-card">
                <span className="step-num">3</span>
                <h3>Prove it is rebuildable</h3>
                <p>
                  Each project's own package manager is asked to confirm the
                  lockfile, read-only. A cleanup never rewrites a file Git
                  tracks — it reports the problem and leaves the tree alone.
                </p>
              </div>
              <div className="step-card">
                <span className="step-num">4</span>
                <h3>Delete, then restore on demand</h3>
                <p>
                  Only verified directories are removed. Coming back to the
                  project later is as simple as <code>devp restore</code>, and
                  every project in the tree comes back with its own manager.
                </p>
              </div>
            </div>
          </div>
        </Reveal>

        {/* ------------------------------ safety ------------------------------ */}
        <Reveal as="section" className="section bg-card-section" id="safety">
          <div className="container">
            <div className="section-header">
              <h2 className="section-title">
                Seven things it{" "}
                <span className="gradient-text">will not do</span>
              </h2>
              <p className="section-subtitle">
                These are invariants in the code, not conventions in a README.
              </p>
            </div>

            <div className="features-grid">
              <div className="f-card">
                <div className="f-icon">
                  <Lock className="c-green" />
                </div>
                <h3>Delete anything unproven</h3>
                <p>
                  No directory is removed until its package manager confirms a
                  usable lockfile. No flag bypasses this —{" "}
                  <code>--ignore-idle</code> included.
                </p>
              </div>
              <div className="f-card">
                <div className="f-icon">
                  <GitBranch className="c-blue" />
                </div>
                <h3>Leave the .git boundary</h3>
                <p>
                  dev-prune only operates inside a directory containing a valid{" "}
                  <code>.git</code> root, and behaves identically regardless of
                  where you invoked it from.
                </p>
              </div>
              <div className="f-card">
                <div className="f-icon">
                  <Layers className="c-purple" />
                </div>
                <h3>Reach into a nested repository</h3>
                <p>
                  Discovery stops at a nested <code>.git</code>. A submodule is
                  pruned as itself, or not at all — never as part of its parent.
                </p>
              </div>
              <div className="f-card">
                <div className="f-icon">
                  <Link2 className="c-yellow" />
                </div>
                <h3>Follow a symlink or junction</h3>
                <p>
                  A linked bloat directory points at storage the repository does
                  not own, so it is refused outright rather than followed.
                </p>
              </div>
              <div className="f-card">
                <div className="f-icon">
                  <FileCode className="c-cyan" />
                </div>
                <h3>Execute anything from your repo</h3>
                <p>
                  <code>.devprune.json</code> holds inert data only: an ignore
                  flag, an idle-day override, a display name, automation
                  opt-outs. It can never name a command.
                </p>
              </div>
              <div className="f-card">
                <div className="f-icon">
                  <ShieldCheck className="c-green" />
                </div>
                <h3>Guess when it cannot read your config</h3>
                <p>
                  A <code>.devprune.json</code> that will not parse skips the
                  repository and reports the syntax error. The unreadable file
                  may have been the one saying <code>"ignore": true</code>.
                </p>
              </div>
              <div className="f-card">
                <div className="f-icon">
                  <EyeOff className="c-blue" />
                </div>
                <h3>Phone home</h3>
                <p>
                  No analytics, no diagnostics, no usage data, no self-update.
                  One request exists — an unauthenticated release check against
                  GitHub, with no body and no identifier — and{" "}
                  <code>devp config set update_check false</code> ends it.
                </p>
              </div>
              <div className="f-card">
                <div className="f-icon">
                  <Gauge className="c-yellow" />
                </div>
                <h3>Ignore your opt-out slowly</h3>
                <p>
                  <code>ignore.devprune.json</code> in a repository root is a
                  single file-existence check, made before any config is read or
                  parsed.
                </p>
              </div>
              <div className="f-card">
                <div className="f-icon">
                  <Undo2 className="c-purple" />
                </div>
                <h3>Corrupt its own state</h3>
                <p>
                  The registry is written to a temporary file and swapped into
                  place, so an interrupted write cannot leave a half-written
                  list of your repositories.
                </p>
              </div>
            </div>

            <p className="section-footnote">
              Full detail in{" "}
              <a
                href={`${DOCS}/SAFETY_INVARIANTS.md`}
                target="_blank"
                rel="noreferrer"
              >
                Safety Invariants <ExternalLink size={12} />
              </a>
              .
            </p>
          </div>
        </Reveal>

        {/* ---------------------------- ecosystems ---------------------------- */}
        <Reveal as="section" className="section" id="ecosystems">
          <div className="container">
            <div className="section-header">
              <h2 className="section-title">
                Eight managers.{" "}
                <span className="gradient-text">
                  Any number per repository.
                </span>
              </h2>
              <p className="section-subtitle">
                A repository is not assumed to be one project. dev-prune walks
                the root and, by default, six levels below it — raise or lower
                that with <code>devp config set scan_depth N</code> — and every
                directory a package manager recognises is verified, pruned and
                restored on its own terms.
              </p>
            </div>

            {ECOSYSTEMS.map((group) => (
              <EcosystemGroup key={group.id} group={group} />
            ))}

            <div className="mono-grid">
              <div className="mono-card">
                <h4>Three managers, one root</h4>
                <pre>{`monorepo/
├── package-lock.json
├── uv.lock
└── Cargo.toml`}</pre>
              </div>
              <div className="mono-card">
                <h4>One manager each, nested</h4>
                <pre>{`monorepo/
├── frontend/
│   └── pnpm-lock.yaml
├── services/api/
│   └── uv.lock
└── tools/cli/
    └── Cargo.toml`}</pre>
              </div>
              <div className="mono-card">
                <h4>Root plus nested, mixed</h4>
                <pre>{`monorepo/
├── Cargo.toml
├── web/
│   └── package-lock.json
└── scripts/
    └── requirements.txt`}</pre>
              </div>
            </div>

            <div className="info-card eco-contribute">
              <h3>
                <Puzzle size={18} /> Nine would be better than eight
              </h3>
              <p>
                Adding a manager is deliberately small: implement one{" "}
                <code>PackageManager</code> trait — detect, list bloat directories,
                verify the lockfile, restore — register it in one array, and add
                its fixtures to the adapter test suite. Nothing else in the
                codebase has to know it exists. Composer, Gradle, Maven, Bundler,
                CocoaPods, Nix and Gems are all natural fits.
              </p>
              <p className="muted">
                The walkthrough, with the trait signature and a worked example,
                is in{" "}
                <a
                  href={`${DOCS}/ADDING_ADAPTERS.md`}
                  target="_blank"
                  rel="noreferrer"
                >
                  Adding an adapter <ExternalLink size={12} />
                </a>{" "}
                — and{" "}
                <a
                  href={`${REPO}/blob/main/CONTRIBUTING.md`}
                  target="_blank"
                  rel="noreferrer"
                >
                  CONTRIBUTING.md <ExternalLink size={12} />
                </a>{" "}
                covers what a pull request needs before it can be merged.
              </p>
            </div>
          </div>
        </Reveal>

        {/* ---------------------------- calculator ---------------------------- */}
        <Reveal
          as="section"
          className="section bg-card-section"
          id="calculator"
        >
          <div className="container">
            <div className="section-header">
              <h2 className="section-title">
                Rough <span className="gradient-text">back-of-envelope</span>
              </h2>
              <p className="section-subtitle">
                Not a measurement — your numbers, multiplied. Run{" "}
                <code>devp run --dry-run</code> for the real figure.
              </p>
            </div>

            <div className="calculator-box">
              <div className="calc-sliders">
                <div className="slider-group">
                  <label className="slider-label" htmlFor="calc-repos">
                    <span>Local Git repositories</span>
                    <span className="val-badge">{projectsCount}</span>
                  </label>
                  <input
                    id="calc-repos"
                    type="range"
                    min="3"
                    max="120"
                    value={projectsCount}
                    onChange={(e) => setProjectsCount(Number(e.target.value))}
                  />
                </div>
                <div className="slider-group">
                  <label className="slider-label" htmlFor="calc-size">
                    <span>Average dependency tree</span>
                    <span className="val-badge">{avgSizeGB.toFixed(1)} GB</span>
                  </label>
                  <input
                    id="calc-size"
                    type="range"
                    min="0.1"
                    max="5"
                    step="0.1"
                    value={avgSizeGB}
                    onChange={(e) => setAvgSizeGB(Number(e.target.value))}
                  />
                </div>
                <div className="slider-group">
                  <label className="slider-label" htmlFor="calc-idle">
                    <span>Share of them gone quiet</span>
                    <span className="val-badge">{idleShare}%</span>
                  </label>
                  <input
                    id="calc-idle"
                    type="range"
                    min="5"
                    max="100"
                    step="5"
                    value={idleShare}
                    onChange={(e) => setIdleShare(Number(e.target.value))}
                  />
                </div>
              </div>

              <div className="calc-result">
                <div className="res-icon">
                  <HardDrive size={40} />
                </div>
                <div className="res-meta">
                  <span className="res-title">Roughly reclaimable</span>
                  <span className="res-value">{reclaimGB} GB</span>
                </div>
              </div>
            </div>
          </div>
        </Reveal>

        {/* ----------------------------- commands ----------------------------- */}
        <Reveal as="section" className="section" id="commands">
          <div className="container">
            <div className="section-header">
              <h2 className="section-title">
                The whole <span className="gradient-text">command surface</span>
              </h2>
              <p className="section-subtitle">
                <code>dev-prune</code> and <code>devp</code> are the same
                executable under two names. Both work in every shell — cmd,
                PowerShell, bash, fish, an IDE terminal, a scheduled task —
                without a profile alias that has to be re-sourced.
              </p>
              <p className="section-subtitle dot-note">
                Wherever a command takes <code>[PATH]</code>, a literal{" "}
                <code>.</code> means the directory you are standing in. It is
                the default for <code>init</code>, <code>link</code>,{" "}
                <code>unlink</code>, <code>restore</code> and the{" "}
                <code>config</code> per-repo actions, and <code>run</code>{" "}
                accepts it as its target — so <code>devp run .</code> prunes
                this repository and nothing else.
                Worth saying out loud because <code>.</code> is usually treated
                as a shell detail rather than an argument: here it is a real
                path, it works on every platform, and it works the same in every
                command that takes one.
              </p>
            </div>

            <div className="table-scroll">
              <table className="data-table">
                <thead>
                  <tr>
                    <th>Command</th>
                    <th>What it does</th>
                  </tr>
                </thead>
                <tbody>
                  <tr>
                    <td className="td-name">
                      <code>devp init [PATHS]</code>
                    </td>
                    <td>
                      Crawl for Git repositories and register them, then run the
                      setup pass
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp link [PATH]</code>
                    </td>
                    <td>Register a single repository</td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp unlink [PATH]</code>
                    </td>
                    <td>Unregister a repository. Deletes nothing on disk</td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp unlink --missing</code>
                    </td>
                    <td>
                      Drop every registered path whose directory no longer
                      exists, in one pass
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp undo</code>
                    </td>
                    <td>
                      Revert the most recent <code>init</code> or{" "}
                      <code>link</code>
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp run [PATH]</code>
                    </td>
                    <td>Prune every registered repository, or one target</td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp status [--top N]</code>
                    </td>
                    <td>
                      Interactive dashboard; a plain table when there is no TTY.{" "}
                      <code>--top N</code> lists only the N repositories with the
                      most reclaimable space — the totals above the table still
                      cover every one of them
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp status --drift</code>
                    </td>
                    <td>
                      Every environment holding packages its lockfile never
                      recorded, with the one command that records them. A pure
                      read — this is what a prune would refuse on
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp stats</code>
                    </td>
                    <td>
                      What has already been reclaimed: lifetime total, prune
                      passes, the most recent pass and how to undo it, and the
                      repositories that gave back the most
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp completions &lt;shell&gt;</code>
                    </td>
                    <td>
                      Print a tab-completion script for bash, zsh, fish,
                      PowerShell or elvish, generated from the same argument
                      definition the binary parses with
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp caches</code>
                    </td>
                    <td>
                      Size every package manager cache on the machine — npm to
                      cargo to Maven, Gradle, NuGet, vcpkg and Conan — and print
                      the command that clears each. Reports only — it deletes
                      nothing
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp restore [PATH]</code>
                    </td>
                    <td>Reinstall dependencies for every project in a tree</td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp restore --last-run</code>
                    </td>
                    <td>
                      Put back exactly what the last prune pass deleted, in every
                      repository it touched
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp doctor [PATH]</code>
                    </td>
                    <td>
                      Check the installation, or one repository — ending with the
                      single reason a pass would or would not touch it
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp doctor --fix</code>
                    </td>
                    <td>
                      Repair what the checks found — a stale <code>devp</code>{" "}
                      twin, hooks or a scheduler pointing at a binary that is
                      gone, dead registry entries. Mends installed-but-broken
                      only; never a first-time install
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp config …</code>
                    </td>
                    <td>
                      Global settings, per-repo config, scheduler, hooks, file
                      manager icons. <code>devp config wizard</code> walks every
                      setting one at a time
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp setup [--status]</code>
                    </td>
                    <td>
                      Install any missing integration; <code>--status</code>{" "}
                      only reports
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp skill</code>
                    </td>
                    <td>
                      Export <code>SKILL.md</code> and install it into your AI
                      agent's skills directory
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp update</code>
                    </td>
                    <td>Print the installed version and the upgrade command</td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp uninstall [--deep]</code>
                    </td>
                    <td>
                      Remove the program — scheduler, hooks, agent skill, PATH
                      entry and every copy of the binary, install channels
                      included; <code>--deep</code> also clears config
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp -V</code>
                    </td>
                    <td>
                      Version plus an environment audit: OS, arch, config path,
                      PATH
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>

            <div className="split-grid">
              <div className="info-card">
                <h3>
                  <Terminal size={18} /> Global flags
                </h3>
                <ul>
                  <li>
                    <code>--dry-run</code> — report every candidate and its
                    size; touch nothing
                  </li>
                  <li>
                    <code>--ignore-idle</code> — lift the idle-day wait, and
                    only that. Lockfile verification still applies
                  </li>
                  <li>
                    <code>-y</code> / <code>--yes</code> — skip interactive
                    confirmation
                  </li>
                </ul>
                <p className="muted">
                  On <code>run</code>: <code>--except</code> keeps named
                  repositories out of the pass, <code>--only</code> /{" "}
                  <code>--skip</code> narrow it to certain managers,{" "}
                  <code>--min-size</code> sets a size floor, and{" "}
                  <code>--json</code> emits one machine-readable document
                  instead of the report, on <code>run</code>,{" "}
                  <code>status</code>, <code>stats</code> and{" "}
                  <code>caches</code>.
                </p>
              </div>
              <div className="info-card">
                <h3>
                  <Gauge size={18} /> Exit codes
                </h3>
                <ul>
                  <li>
                    <code>0</code> — success, including "nothing was idle enough
                    to prune"
                  </li>
                  <li>
                    <code>1</code> — the command failed; the reason is on stderr
                  </li>
                  <li>
                    <code>2</code> — the arguments were not usable
                  </li>
                </ul>
              </div>
              <div className="info-card">
                <h3>
                  <Cpu size={18} /> Background automation
                </h3>
                <p>
                  The OS scheduler and Git auto-registration hooks install
                  themselves at install time, and again after an upgrade if
                  anything went missing. The pass skips the hooks when{" "}
                  <code>git</code> is absent, and when{" "}
                  <code>core.hooksPath</code> already belongs to husky,
                  pre-commit or lefthook it says so instead of taking the slot —{" "}
                  <code>devp hook install --chain</code> takes it politely, by
                  forwarding every hook on to the tool that had it.
                </p>
                <p className="muted">
                  Off switches: <code>auto_daemon</code>,{" "}
                  <code>auto_hooks</code>, <code>auto_setup</code>, or{" "}
                  <code>DEV_PRUNE_NO_AUTO_SETUP=1</code>.
                </p>
              </div>
              <div className="info-card">
                <h3>
                  <FileCode size={18} /> Per-repository config
                </h3>
                <pre>{`{
  "project_name": "My App",
  "ignore": false,
  "override_idle_days": 30,
  "disable_daemon": false,
  "disable_hooks": false
}`}</pre>
                <p className="muted">
                  Or drop an empty <code>ignore.devprune.json</code> in the root
                  to opt out entirely.
                </p>
              </div>
            </div>

            <p className="section-footnote">
              Every flag, alias and shorthand is in the{" "}
              <a
                href={`${DOCS}/CLI_REFERENCE.md`}
                target="_blank"
                rel="noreferrer"
              >
                CLI reference <ExternalLink size={12} />
              </a>
              .
            </p>
          </div>
        </Reveal>

        {/* -------------------------------- AI -------------------------------- */}
        <Reveal as="section" className="section bg-card-section" id="ai">
          <div className="container">
            <div className="section-header">
              <h2 className="section-title">
                Your agent already{" "}
                <span className="gradient-text">knows how to use it</span>
              </h2>
              <p className="section-subtitle">
                dev-prune ships a skill file describing its full command
                surface, its safety rules, its exit codes and a troubleshooting
                decision tree. It is exported to your config directory
                automatically and kept in step with the installed binary.
              </p>
            </div>

            <div className="split-grid">
              <div className="info-card">
                <h3>
                  <Bot size={18} /> Export it
                </h3>
                <div className="cmd-line small">
                  <span className="cmd-prompt">$</span>
                  <code className="cmd-code">devp skill</code>
                  <CopyButton
                    id="skill"
                    text="devp skill"
                    copied={copiedKey}
                    onCopy={handleCopy}
                  />
                </div>
                <p className="muted">
                  Writes <code>SKILL.md</code> to the config directory and
                  prints ready-to-paste onboarding prompts for Claude Code,
                  Cursor, Windsurf, Copilot, Antigravity and anything else that
                  reads a skill file.
                </p>
              </div>
              <div className="info-card">
                <h3>
                  <Sparkles size={18} /> Just ask
                </h3>
                <p>
                  “How much space can I get back?” → the agent runs{" "}
                  <code>devp run --dry-run</code> and reads you the total.
                  “Clean up but keep the API project” → it runs{" "}
                  <code>devp run --except api</code>, which never verifies,
                  deletes or reinstalls that one, rather than pruning everything
                  and downloading it back afterwards. “Why didn't it delete
                  anything?” → it runs <code>devp doctor .</code>, which ends by
                  naming the one reason, and tells you.
                </p>
                <p className="muted">
                  You do not have to learn the flags. That is the point.
                </p>
              </div>
              <div className="info-card">
                <h3>
                  <ExternalLink size={18} /> Machine-readable docs
                </h3>
                <ul>
                  <li>
                    <a href="/llms.txt">/llms.txt</a> — summary for language
                    models
                  </li>
                  <li>
                    <a
                      href={`${REPO}/blob/main/.agents/skills/dev-prune/SKILL.md`}
                      target="_blank"
                      rel="noreferrer"
                    >
                      SKILL.md
                    </a>{" "}
                    — the shipped skill, verbatim
                  </li>
                  <li>
                    <a href="/schemas/v1/devprune.schema.json">
                      devprune.schema.json
                    </a>{" "}
                    — JSON Schema for <code>.devprune.json</code>
                  </li>
                </ul>
              </div>
            </div>
          </div>
        </Reveal>

        {/* -------------------------------- FAQ -------------------------------- */}
        <Reveal as="section" className="section" id="faq">
          <div className="container narrow">
            <div className="section-header">
              <h2 className="section-title">
                Questions worth{" "}
                <span className="gradient-text">asking first</span>
              </h2>
            </div>

            <div className="faq-list">
              <Faq q="Can it delete something I cannot get back?">
                No directory is removed until its package manager has confirmed
                a usable lockfile. If verification fails, the directory is left
                alone and dev-prune prints the exact command to fix it. Nothing
                bypasses this — not <code>--ignore-idle</code>, not{" "}
                <code>-y</code>, not the daemon.
              </Faq>
              <Faq q="There used to be a --force. Where did it go?">
                It is now <code>--ignore-idle</code>, which is what it always
                actually did: lift the idle-day wait, and nothing else. The old
                spelling was misleading — “force” reads like “override the
                safety checks”, and there is no flag that does that. Typing{" "}
                <code>--force</code> still works and prints a one-line note
                pointing at the new name, along with the usual reasons a
                directory was skipped and how to fix each one.
              </Faq>
              <Faq q="What about uncommitted work?">
                Idleness is judged from <code>git log</code> <em>and</em> source
                file modification times. A repository you edited yesterday
                without committing is Active, and is not a candidate.
              </Faq>
              <Faq q="Does it work on monorepos?">
                That is the design. Every package-manager project inside a
                repository is discovered — six levels deep by default,{" "}
                <code>devp config set scan_depth N</code> to change it — and
                handled independently, and each directory is reported by its
                repository-relative path.
              </Faq>
              <Faq q="Will it break husky or pre-commit?">
                No. dev-prune's Git hooks use the global{" "}
                <code>core.hooksPath</code>, and Git allows exactly one hooks
                directory with no way to chain them. If that setting already
                belongs to another tool, the setup pass leaves it alone and says
                so rather than silently taking the slot. When you want both,{" "}
                <code>devp hook install --chain</code> claims the slot and
                writes a shim for every hook the displaced directory has, each
                one running dev-prune's registration and then{" "}
                <code>exec</code>-ing the original — so husky still fires, and{" "}
                <code>devp hook uninstall</code> puts the old path back exactly
                as it was. If the other tool later adds a hook, the next setup
                pass notices the drift and rebuilds the shims.
              </Faq>
              <Faq q="How do I stop it installing background things?">
                It already stops itself in the places that would be wrong: a CI
                runner, a container, or any non-interactive session is detected
                and the pass is skipped without being asked. Otherwise{" "}
                <code>devp config set auto_setup false</code> turns off the
                whole pass, <code>auto_hooks</code> and <code>auto_daemon</code>{" "}
                turn off one part each, and{" "}
                <code>DEV_PRUNE_NO_AUTO_SETUP=1</code> overrides all three
                without a config file — useful in a Dockerfile, where you can
                set it before the binary is ever run.
              </Faq>
              <Faq q="Does it send anything over the network?">
                No analytics, no diagnostics, no usage data — none collected,
                none sent. There is exactly one request: an unauthenticated{" "}
                <code>GET</code> to GitHub's public releases endpoint, so it can
                tell you when a newer version is out. It has no body, carries no
                identifier, and runs at most once a week. It is opt-out rather
                than opt-in, because a tool that deletes directories is one
                whose fixes you want:{" "}
                <code>devp config set update_check false</code> switches it off,
                and <code>devp update --offline</code> skips it once. Everything
                else on the wire is your own package manager during verification
                or restore.
              </Faq>
              <Faq q="How do I remove it?">
                <code>devp uninstall</code> removes the program: the scheduler,
                hooks, the installed agent skill, the PATH entry and the
                binaries themselves — then finds every other copy that pip,
                npm, cargo or uv left behind and removes them all after one
                confirmation, printing each manager's own uninstall line so
                its records clear too. On Windows the last files disappear a
                few seconds after the command exits — no reboot needed. Add{" "}
                <code>--deep</code> to also wipe the configuration directory and
                every registered repository's <code>.devprune.json</code> — it
                asks first.
              </Faq>
            </div>
          </div>
        </Reveal>

        {/* ------------------------------- CTA ------------------------------- */}
        <Reveal as="section" className="section cta-section">
          <div className="container narrow center">
            <h2 className="section-title">
              Try it in dry-run. It deletes nothing.
            </h2>
            <div className="cta-cmds">
              <div className="cmd-line">
                <span className="cmd-prompt">$</span>
                <code className="cmd-code">
                  {installCommands[installTab].cmd}
                </code>
                <CopyButton
                  id={`cta-${installTab}`}
                  text={installCommands[installTab].cmd}
                  copied={copiedKey}
                  onCopy={handleCopy}
                />
              </div>
              <div className="cmd-line">
                <span className="cmd-prompt">$</span>
                <code className="cmd-code">
                  devp init ~/Code &amp;&amp; devp run --dry-run
                </code>
                <CopyButton
                  id="cta-run"
                  text="devp init ~/Code && devp run --dry-run"
                  copied={copiedKey}
                  onCopy={handleCopy}
                />
              </div>
            </div>
            <p className="muted">
              Apache-2.0 · no analytics · Windows, macOS and Linux · Rust 1.88
            </p>
          </div>
        </Reveal>
      </main>

      <footer className="footer">
        <div className="container footer-content">
          <div className="footer-brand">
            <img
              src="/assets/icon_small.png"
              alt=""
              width="24"
              height="24"
            />
            <span>dev-prune (devp)</span>
            <span className="footer-tagline">
              · lockfile-safe workspace pruner
            </span>
          </div>
          <nav className="footer-links" aria-label="Footer">
            <a href={REPO} target="_blank" rel="noreferrer">
              GitHub
            </a>
            <a href={`${DOCS}/README.md`} target="_blank" rel="noreferrer">
              Docs
            </a>
            <a
              href={`${DOCS}/CLI_REFERENCE.md`}
              target="_blank"
              rel="noreferrer"
            >
              CLI reference
            </a>
            <a
              href={`${DOCS}/troubleshooting/README.md`}
              target="_blank"
              rel="noreferrer"
            >
              Troubleshooting
            </a>
            <a href={`${REPO}/releases`} target="_blank" rel="noreferrer">
              Releases
            </a>
            <a
              href={`${REPO}/blob/main/LICENSE.md`}
              target="_blank"
              rel="noreferrer"
            >
              Apache-2.0
            </a>
          </nav>
        </div>
        <div className="container">
          <p className="footer-legal">
            Copyright 2026 VKrishna04 · Licensed under the Apache License,
            Version 2.0
          </p>
        </div>
      </footer>
    </div>
  );
}
