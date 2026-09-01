// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

import React, { useState, useEffect, useRef } from "react";
import { useInView, useReducedMotion } from "framer-motion";
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

import ReclaimLedger from "./ledger.jsx";
import Languages from "./languages.jsx";

const REPO = "https://github.com/Life-Experimentalist/dev-prune";
const PORTFOLIO = "https://vkrishna04.me";
const MARKETPLACE =
  "https://marketplace.visualstudio.com/items?itemName=VKrishna04.dev-prune";
const OPENVSX = "https://open-vsx.org/extension/VKrishna04/dev-prune";
const DOCS = `${REPO}/blob/main/docs`;
const VERSION = "1.16.0";
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
      meta.setAttribute(
        "content",
        resolved === "light" ? "#fbfcff" : "#10131e",
      );
    if (switchRef.current)
      switchRef.current.setAttribute(
        "aria-checked",
        String(resolved === "light"),
      );
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
  // Framer Motion's viewport hook, not a hand-rolled observer: it already
  // handles the once-only latch, the SSR pass (false on the server, which is
  // what `.js .reveal` in sections.css expects), and teardown.
  const inView = useInView(ref, { once: true, margin: "0px 0px -10% 0px" });
  const reduce = useReducedMotion();
  const shown = inView || reduce;

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
// Nine channels in one flat row of tabs was a wall to read. They divide cleanly by what
// you are actually choosing between: a one-liner that needs nothing installed, a package
// manager you already use, or the archive itself.
const INSTALL_GROUPS = [
  { id: "script", label: "Install script" },
  { id: "manager", label: "Package manager" },
  { id: "manual", label: "Download" },
];

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
      {
        name: "Deno",
        detect: <code>deno.lock</code>,
        deletes: (
          <>
            <code>node_modules</code>, <code>vendor</code>
          </>
        ),
        verify: (
          <>
            <code>deno.lock</code> complete and not stale
          </>
        ),
        restore: <code>deno install</code>,
      },
    ],
    tieBreak: (
      <>
        <strong>When more than one claims the same tree,</strong> the owner is
        decided in this order: the <code>packageManager</code> field in{" "}
        <code>package.json</code>; else whichever manager's bookkeeping files
        are actually inside the installed <code>node_modules</code>; else the
        most recently written lockfile. Only that manager verifies, deletes and
        restores — the others are not consulted. Deno is outside that contest:
        the other four are alternatives to each other, while a repository
        holding both a <code>deno.lock</code> and a{" "}
        <code>package-lock.json</code> genuinely uses both tools. It takes{" "}
        <code>vendor/</code> only when the Deno config asked for one — Go and
        Composer use that name for something else entirely — and the shared{" "}
        <code>node_modules</code> is still counted and deleted once.
      </>
    ),
  },
  {
    id: "eco-python",
    language: "Python",
    summary:
      "Five managers that can describe the same project, because a uv, Poetry or PDM project is still a directory with a virtual environment in it.",
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
        name: "Poetry",
        detect: (
          <>
            <code>poetry.lock</code>, <code>[tool.poetry]</code>
          </>
        ),
        deletes: <code>.venv</code>,
        verify: <code>poetry check --lock</code>,
        restore: <code>poetry install</code>,
      },
      {
        name: "PDM",
        detect: (
          <>
            <code>pdm.lock</code>, <code>[tool.pdm]</code>
          </>
        ),
        deletes: (
          <>
            <code>.venv</code>, <code>__pypackages__</code>
          </>
        ),
        verify: <code>pdm lock --check</code>,
        restore: <code>pdm install</code>,
      },
      {
        name: "Pipenv",
        detect: <code>Pipfile</code>,
        deletes: (
          <>
            <code>.venv</code>, in-project only
          </>
        ),
        verify: <code>pipenv verify</code>,
        restore: <code>pipenv install --deploy</code>,
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
        <strong>uv and Poetry win over plain venv</strong> whenever their
        lockfile or <code>pyproject.toml</code> table is present, because a real
        lockfile rebuilds the environment exactly and a{" "}
        <code>requirements.txt</code> only approximates it. When uv and Poetry
        both describe the same project — usually a half-finished migration — the
        one whose lockfile is actually on disk owns the environment. A project
        with none of that falls back to venv, and a venv with an empty{" "}
        <code>requirements.txt</code> is refused outright — there would be
        nothing to reinstall from. Pipenv is claimed only when its environment
        is <em>inside</em> the repository — the <code>.venv</code> you get from{" "}
        <code>PIPENV_VENV_IN_PROJECT</code>. Its default is a virtualenv in a
        shared directory under your home, keyed by a hash of the project path,
        which a repository-scoped prune has no business reaching into.
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
        name: "Cargo — opt-in",
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
        means the same thing for every adapter with a lockfile.{" "}
        <strong>Cargo ships off.</strong> <code>target/</code> is compiler
        output: the lockfile proves it comes back, but it comes back by
        recompiling rather than downloading, which on a real workspace is
        minutes rather than seconds.{" "}
        <code>devp config set enable_cargo true</code> turns it on, under the
        same <code>build_idle_days</code> gate as the build tools below.
      </>
    ),
  },
  {
    id: "eco-more",
    language: "PHP, Ruby, Elixir & Apple",
    summary:
      "Four managers with one thing in common: only the copy that lives inside the repository is ever claimed.",
    rows: [
      {
        name: "Composer",
        detect: <code>composer.json</code>,
        deletes: <code>vendor</code>,
        verify: <code>composer validate --no-check-publish</code>,
        restore: <code>composer install</code>,
      },
      {
        name: "Bundler",
        detect: <code>Gemfile</code>,
        deletes: <code>vendor/bundle</code>,
        verify: <code>bundle lock --check</code>,
        restore: <code>bundle install</code>,
      },
      {
        name: "CocoaPods",
        detect: <code>Podfile</code>,
        deletes: <code>Pods</code>,
        verify: (
          <>
            <code>Podfile.lock</code> complete and not stale
          </>
        ),
        restore: <code>pod install</code>,
      },
      {
        name: "Mix",
        detect: <code>mix.exs</code>,
        deletes: <code>deps</code>,
        verify: (
          <>
            <code>mix.lock</code> complete and not stale
          </>
        ),
        restore: <code>mix deps.get</code>,
      },
    ],
    tieBreak: (
      <>
        <strong>Only what is inside the repository.</strong> Bundler defaults to
        a shared gem home outside it, so <code>vendor/bundle</code> is claimed
        and nothing else — and <code>.bundle/</code> is deliberately left alone,
        because it holds the path configuration that sends the next{" "}
        <code>bundle install</code> back there. Composer declines{" "}
        <code>vendor/</code> outright when a <code>vendor/bundle</code> is
        sitting inside it: deleting it would take gems with it under a proof
        that says nothing about them. Mix takes <code>deps/</code> and never{" "}
        <code>_build/</code>, which is compiled output. CocoaPods and Mix also
        share the offline proof with the Dart adapter — none of the three
        ecosystems has a read-only in-sync check, and the write-side commands
        fix drift by re-downloading, so what is verified instead is that the
        lockfile is structurally complete and no older than the manifest it
        came from.
      </>
    ),
  },
  {
    id: "eco-infra",
    language: "Infrastructure",
    summary:
      "One adapter, and the interesting part is everything it refuses to claim.",
    rows: [
      {
        name: "Terraform",
        detect: (
          <>
            any <code>*.tf</code> / <code>*.tf.json</code>
          </>
        ),
        deletes: <code>.terraform/providers</code>,
        verify: (
          <>
            <code>.terraform.lock.hcl</code> records a provider
          </>
        ),
        restore: <code>terraform init -backend=false</code>,
      },
    ],
    tieBreak: (
      <>
        <strong>
          Providers only, never the rest of <code>.terraform/</code>.
        </strong>{" "}
        <code>environment</code> records the workspace you selected, and losing
        it silently returns you to <code>default</code> — so the next{" "}
        <code>apply</code> somebody runs without looking targets the wrong
        environment. <code>terraform.tfstate</code> in there is the
        backend&apos;s initialisation record, and rebuilding it needs the
        backend&apos;s credentials. <code>modules/</code> is fetched from module
        sources that <code>.terraform.lock.hcl</code> says nothing about, so an
        unpinned <code>git::</code> source can come back as something else. The
        providers are the bulk anyway — a few hundred megabytes of plugin
        binaries per root module, times every environment directory in the
        repository.
      </>
    ),
  },
  {
    id: "eco-jvm",
    language: "JVM, Swift, Dart & C/C++ — opt-in",
    summary:
      "Build tools whose recoverability proof is the manifest itself: a build tree is regenerated by recompiling the sources sitting next to it, not by downloading.",
    rows: [
      {
        name: "Gradle",
        detect: (
          <>
            <code>build.gradle[.kts]</code>, <code>settings.gradle[.kts]</code>
          </>
        ),
        deletes: (
          <>
            <code>build</code>, <code>.gradle</code>
          </>
        ),
        verify: <>manifest present and readable</>,
        restore: (
          <>
            next <code>./gradlew build</code>
          </>
        ),
      },
      {
        name: "Maven",
        detect: <code>pom.xml</code>,
        deletes: <code>target</code>,
        verify: (
          <>
            <code>pom.xml</code> parses as a Maven manifest
          </>
        ),
        restore: (
          <>
            next <code>mvn package</code>
          </>
        ),
      },
      {
        name: "SwiftPM",
        detect: <code>Package.swift</code>,
        deletes: <code>.build</code>,
        verify: (
          <>
            <code>Package.swift</code> declares a package
          </>
        ),
        restore: (
          <>
            next <code>swift build</code>
          </>
        ),
      },
      {
        name: "Dart / Flutter",
        detect: <code>pubspec.yaml</code>,
        deletes: <code>.dart_tool</code>,
        verify: (
          <>
            <code>pubspec.lock</code> complete and not stale
          </>
        ),
        restore: (
          <>
            <code>dart pub get</code> / <code>flutter pub get</code>
          </>
        ),
      },
      {
        name: "Elixir Mix _build",
        detect: <code>mix.exs</code>,
        deletes: <code>_build</code>,
        verify: (
          <>
            <code>mix.exs</code> and <code>mix.lock</code> both present
          </>
        ),
        restore: (
          <>
            next <code>mix compile</code>
          </>
        ),
      },
      {
        name: "vcpkg",
        detect: <code>vcpkg.json</code>,
        deletes: <code>vcpkg_installed</code>,
        verify: (
          <>
            <code>vcpkg.json</code> declares dependencies
          </>
        ),
        restore: (
          <>
            next <code>vcpkg install</code>
          </>
        ),
      },
      {
        name: "cmake_build",
        detect: <code>CMakeLists.txt</code>,
        deletes: (
          <>
            any tree holding a <code>CMakeCache.txt</code>
          </>
        ),
        verify: (
          <>
            that cache names a source directory in this repository
          </>
        ),
        restore: (
          <>
            next <code>cmake --build</code>
          </>
        ),
      },
    ],
    tieBreak: (
      <>
        <strong>Off until you switch them on.</strong> A build directory takes
        far longer to get back than a dependency directory — a full recompile,
        not a download — so these seven, and Cargo above, ship disabled and
        invisible. <code>devp config set enable_gradle true</code> /{" "}
        <code>enable_maven true</code> / <code>enable_swift true</code> /{" "}
        <code>enable_dart true</code> / <code>enable_mix_build true</code> /{" "}
        <code>enable_vcpkg true</code> / <code>enable_cmake_build true</code>{" "}
        turns them on, and their candidates wait
        for <code>build_idle_days</code> (45 by default), applied as the{" "}
        <em>maximum</em> of it and <code>idle_days</code> — the build-tool gate
        can only ever make pruning later, never earlier. One adapter can be made
        to wait longer than the rest with{" "}
        <code>devp config set adapter_idle_days cargo=90</code>, which raises
        that adapter&apos;s window and never lowers it. The dependencies
        themselves never lived in the repository: <code>~/.m2</code>,{" "}
        <code>~/.gradle</code> and <code>~/.pub-cache</code> are machine-wide
        stores, which is <code>devp caches</code> territory. Dart is here for
        the same reason as the others and not an obvious one:{" "}
        <code>.dart_tool/</code> holds a second&apos;s worth of pub metadata
        and, beside it, the <code>build_runner</code> and{" "}
        <code>flutter_build</code> caches, which come back only by recompiling.
        vcpkg is here because it builds every port from source: what sits in{" "}
        <code>vcpkg_installed/</code> is a compiled Boost or Qt, and{" "}
        <code>vcpkg install</code> compiles it again. It reads{" "}
        <code>vcpkg.json</code> for a <code>dependencies</code> list before
        touching anything, because every vcpkg <em>port</em> carries a{" "}
        <code>vcpkg.json</code> too and a port manifest rebuilds nothing. Only
        manifest mode is claimed — classic mode&apos;s one install tree is
        shared by every project on the machine, so it belongs to{" "}
        <code>devp caches</code>. CMake is the one that has to prove whose{" "}
        <code>build/</code> it is, and the answer is never the name.{" "}
        <code>cmake</code> writes a <code>CMakeCache.txt</code> at the top of
        every tree it configures and nobody writes one by hand; that file
        records <code>CMAKE_HOME_DIRECTORY</code>, the source directory it was
        configured from. A directory is claimed only when that recorded source
        directory still exists, still holds a <code>CMakeLists.txt</code> and
        sits inside this repository — so a <code>build/</code> you filled by
        hand is never touched. The search stops at the first cache it finds, so
        the sub-builds <code>FetchContent</code> leaves under{" "}
        <code>build/_deps/</code> go with the tree that configured them, and it
        looks three levels down because Visual Studio configures into{" "}
        <code>out/build/&lt;preset&gt;/</code>.
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

/* ------------------------------------------------------------------ */
/* Motion wiring.                                                       */
/*                                                                      */
/* Everything decorative that needs a number from the browser lives      */
/* here, in one effect, so there is exactly one scroll listener and one  */
/* pointer listener on the page rather than one per card. All of it runs */
/* after hydration and none of it changes the markup, so the prerendered */
/* HTML the crawler reads is byte-identical to what React expects.       */
/*                                                                      */
/* `prefers-reduced-motion` is checked here as well as in motion.css:    */
/* the CSS can stop an animation, but only this can stop the work that   */
/* feeds it.                                                             */
/* ------------------------------------------------------------------ */
const SPOTLIT = ".info-card, .step-card, .eco-group, .f-card, .mono-card";

function useMotion() {
  useEffect(() => {
    const reduced =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    const bar = document.querySelector(".scroll-progress");
    const nav = document.querySelector(".navbar");
    if (bar && !reduced) bar.classList.add("is-live");

    // One rAF-coalesced handler for both the progress bar and the navbar, because
    // they answer the same question and reading scrollHeight twice per frame is a
    // forced layout twice per frame.
    let frame = 0;
    const measure = () => {
      frame = 0;
      const doc = document.documentElement;
      const max = doc.scrollHeight - window.innerHeight;
      const y = window.scrollY;
      if (bar)
        bar.style.setProperty("--sp", max > 0 ? (y / max).toFixed(4) : "0");
      if (nav) nav.classList.toggle("is-condensed", y > 80);
    };
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(measure);
    };

    // The spotlight follows the pointer inside whichever card it is over. Delegated
    // from the document so cards added later need no wiring, and skipped entirely
    // when the pointer is not over one — which is most of the time.
    let mx = 0;
    let my = 0;
    let lit = null;
    let litFrame = 0;
    const paint = () => {
      litFrame = 0;
      if (!lit) return;
      const r = lit.getBoundingClientRect();
      lit.style.setProperty("--mx", `${mx - r.left}px`);
      lit.style.setProperty("--my", `${my - r.top}px`);
    };
    const onMove = (e) => {
      const card =
        e.target instanceof Element ? e.target.closest(SPOTLIT) : null;
      if (!card) {
        lit = null;
        return;
      }
      lit = card;
      mx = e.clientX;
      my = e.clientY;
      if (!litFrame) litFrame = requestAnimationFrame(paint);
    };

    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll, { passive: true });
    measure();
    if (!reduced)
      document.addEventListener("pointermove", onMove, { passive: true });

    return () => {
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
      document.removeEventListener("pointermove", onMove);
      if (frame) cancelAnimationFrame(frame);
      if (litFrame) cancelAnimationFrame(litFrame);
    };
  }, []);

  // Scroll-spy. The nav already links to every section; this only marks which of
  // them you are in. The rootMargin keeps the band in the middle of the viewport,
  // so the mark changes when a section takes over the screen rather than the
  // instant its first pixel appears.
  useEffect(() => {
    if (typeof IntersectionObserver === "undefined") return;
    const links = Array.from(
      document.querySelectorAll('.nav-links a[href^="#"]'),
    );
    if (!links.length) return;
    const byId = new Map(
      links.map((a) => [a.getAttribute("href").slice(1), a]),
    );
    const io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          links.forEach((a) => a.classList.remove("is-current"));
          byId.get(entry.target.id)?.classList.add("is-current");
        }
      },
      { rootMargin: "-45% 0px -50% 0px" },
    );
    for (const id of byId.keys()) {
      const el = document.getElementById(id);
      if (el) io.observe(el);
    }
    return () => io.disconnect();
  }, []);
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

  useMotion();

  // Windows visitors get the PowerShell line first. Runs after hydration, so the
  // prerendered HTML is identical for everyone and the crawler sees a real default.
  useEffect(() => {
    const ua = (navigator.userAgent || "").toLowerCase();
    if (ua.includes("windows")) setInstallTab("powershell");
  }, []);

  const reclaimGB = (projectsCount * avgSizeGB * (idleShare / 100)).toFixed(1);

  const installCommands = {
    bash: {
      group: "script",
      label: "Linux / macOS",
      registry: "Downloads the release archive from GitHub, checks its .sha256, and installs it. No package registry involved.",
      note: "Needs a Unix shell — also fine on Windows under Git Bash, MSYS2, Cygwin or WSL. In PowerShell or Command Prompt it fails with 'sh is not recognized'; use the Windows tabs there.",
      cmd: "curl -fsSL https://devprune.vkrishna04.me/install.sh | sh",
    },
    powershell: {
      group: "script",
      label: "Windows",
      registry: "Downloads the release archive from GitHub, checks its .sha256, and installs it. No package registry involved.",
      note: "Installs to %APPDATA%\\dev-prune\\bin and registers devp for PowerShell and cmd alike.",
      cmd: "iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex",
    },
    cmdexe: {
      group: "script",
      label: "Windows (cmd)",
      registry: "Downloads the release archive from GitHub, checks its .sha256, and installs it. No package registry involved.",
      note: "Command Prompt has no iwr, so it borrows PowerShell for the download. Same install; devp resolves in the next Command Prompt you open.",
      cmd: 'powershell -NoProfile -ExecutionPolicy Bypass -Command "iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex"',
    },
    brew: {
      group: "manager",
      label: "Homebrew",
      registry: "Homebrew tap — Life-Experimentalist/homebrew-tap.",
      note: "macOS and Linux, Intel and ARM. The fully-qualified name taps as it installs, so there is no separate brew tap step — and because the formula belongs to a tap, brew upgrade keeps finding new versions. Installs both names and generates your shell completions from the binary.",
      cmd: "brew install Life-Experimentalist/tap/dev-prune",
    },
    scoop: {
      group: "manager",
      label: "Scoop",
      registry: "Scoop bucket — Life-Experimentalist/scoop-bucket.",
      note: "64-bit, ARM and 32-bit Windows. The manifest carries the download hash, so there is nothing left to trust, and the bucket is what makes scoop update dev-prune work later. Registers both dev-prune and devp.",
      cmd: "scoop bucket add life-experimentalist https://github.com/Life-Experimentalist/scoop-bucket; scoop install dev-prune",
    },
    winget: {
      group: "manager",
      label: "WinGet",
      registry: "The winget-pkgs community repository.",
      note: "Submitted and awaiting a Microsoft reviewer — every winget-pkgs version is a pull request a person signs off, so this command starts resolving when that merges, and not before. WinGet installs the dev-prune name; the devp twin appears the first time you run it.",
      cmd: "winget install VKrishna04.dev-prune",
    },
    npm: {
      group: "manager",
      label: "npm",
      registry: "npm registry — which is also what bun, pnpm and yarn read, so all four install the same published package. dev-prune notices which one you used and upgrades and removes with that one.",
      note: "One small package that pulls in the single platform binary matching your machine — no postinstall download, so it works under npm ci --ignore-scripts and behind a registry mirror. Swap in npx dev-prune status to run it once without installing. Windows needs 1.8.0 or later. bun add -g dev-prune, pnpm add -g dev-prune and yarn global add dev-prune install the same package, and dev-prune treats each as its own channel: a copy bun installed is upgraded and removed with bun.",
      cmd: "npm install -g dev-prune",
    },
    python: {
      group: "manager",
      label: "uv / pipx",
      registry: "PyPI — which uv, pipx and pip all read, so one upload serves the three of them.",
      note: "Platform wheels carrying the binary. Nothing Python runs. Swap in uvx dev-prune status to run it once and leave nothing behind, or pipx install dev-prune.",
      cmd: "uv tool install dev-prune",
    },
    pip: {
      group: "manager",
      label: "pip",
      registry: "PyPI — the same wheels uv tool and pipx install.",
      note: "The same wheels, into whichever environment is active — a venv's Scripts/bin rather than a shared tool directory. Use pip install --user dev-prune for a machine-wide install.",
      cmd: "pip install dev-prune",
    },
    cargo: {
      group: "manager",
      label: "Cargo",
      registry: "crates.io. binstall skips it for the GitHub release archive.",
      note: "crates.io stores source, not binaries, so cargo install always compiles (Rust 1.88+). cargo binstall downloads the same prebuilt archive the installers use.",
      cmd: "cargo binstall dev-prune",
    },
    release: {
      group: "manual",
      label: "Release binary",
      registry: "GitHub Releases — the source every other channel here ultimately points at.",
      note: "Every archive ships a .sha256 sidecar; the installers refuse to run without one. Each release is also attested — gh attestation verify proves it came from this repository's workflow.",
      cmd: "https://github.com/Life-Experimentalist/dev-prune/releases/latest",
    },
  };

  const channelsInGroup = (group) =>
    Object.entries(installCommands).filter(([, v]) => v.group === group);
  const activeGroup = installCommands[installTab].group;

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
   If I do not have a list, run \`devp init --auto --dry-run\`, read me what it found, and
   register it with \`devp init --auto\` once I say yes. Do not register directories I did
   not name or approve. \`devp init\` only records a directory; it never deletes anything
   on its own.

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

      <div className="scroll-progress" aria-hidden="true"></div>
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
            {/* `nav-sec` marks the anchors that get dropped first as the
                header narrows. Nine links plus the toggle plus the GitHub
                button did not fit a 1200px container, so the brand wrapped
                and the version pill landed in the middle of the nav. */}
            <a href="#how">How it works</a>
            <a href="#safety">Safety</a>
            <a href="#ecosystems" className="nav-sec">
              Ecosystems
            </a>
            <a href="#commands" className="nav-ter">Commands</a>
            <a href="#ai" className="nav-sec">
              AI agents
            </a>
            <a href="#editors" className="nav-sec">
              Editors
            </a>
            <a href="/blog/" className="nav-ter">Guides</a>
            <a href="/reference/" className="nav-ter">Reference</a>
            <a href="#faq" className="nav-sec">
              FAQ
            </a>
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
                Gigabytes back.
                <br />
                <span className="gradient-text">
                  Nothing you can&rsquo;t rebuild.
                </span>
              </h1>
              <p className="hero-description">
                <strong>dev-prune</strong> finds Git repositories you have not
                touched in a while and deletes what their package managers can
                rebuild — <code>node_modules</code>, <code>.venv</code>,{" "}
                <code>target</code>, <code>vendor</code> and the rest, across
                twenty-four managers from npm and pip to Composer, Bundler,
                Mix, CocoaPods and Terraform. Nothing is deleted until the
                package manager itself confirms a lockfile can restore it.
                Verification is not a flag you can turn off.
              </p>

              <p className="hero-description">
                Pruning is one command of several. The same binary restores what
                it removed, sizes and clears the caches package managers keep
                outside your projects, clears what Docker is holding, and shows
                where the disk went drive by drive &mdash; one tool for every
                dependency directory on the machine, instead of one per
                ecosystem.
              </p>

              <p className="hero-alias">
                <strong>
                  The command is <code>dev-prune</code>.
                </strong>{" "}
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
                  {INSTALL_GROUPS.map((g) => (
                    <button
                      key={g.id}
                      aria-pressed={activeGroup === g.id}
                      className={activeGroup === g.id ? "active" : ""}
                      onClick={() => setInstallTab(channelsInGroup(g.id)[0][0])}
                    >
                      {g.label}
                    </button>
                  ))}
                </div>
                {/* Every group's channels are rendered, and the inactive ones are hidden
                    with CSS rather than dropped: the prerendered HTML is what a crawler
                    and a reader without JavaScript get, and "you can install this with
                    Homebrew" should not depend on having clicked the right tab. */}
                {INSTALL_GROUPS.filter(
                  (g) => channelsInGroup(g.id).length > 1,
                ).map((g) => (
                  <div
                    key={g.id}
                    className={
                      activeGroup === g.id
                        ? "widget-channels"
                        : "widget-channels is-hidden"
                    }
                    role="group"
                    aria-label={`${g.label} options`}
                  >
                    {channelsInGroup(g.id).map(([key, v]) => (
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
                ))}
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
                <p className="install-registry">
                  <strong>Where it comes from:</strong>{" "}
                  {installCommands[installTab].registry}
                </p>
              </div>
            </div>

            {/* --------------------------- the proof --------------------------- */}
            {/* The ledger first, the terminal under it. On a wide screen this
                column used to begin roughly 500px below the headline, which
                left the most interesting thing on the page under the fold. */}
            <div className="hero-proof">
              <ReclaimLedger />

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
                        tools/cli/target (1.41 GB) [cargo]
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
                        [↑/↓/j/k] Navigate [s] Sort [f] Filter [/] Search [p]
                        Prune [i] Ignore [q] Quit
                      </div>
                      <div className="term-line c-dim">
                        <a href="/">dev-prune</a> · made with ♥ by{" "}
                        <a href={PORTFOLIO} target="_blank" rel="noreferrer">
                          VKrishna04
                        </a>{" "}
                        ·{" "}
                        <a href={REPO} target="_blank" rel="noreferrer">
                          github.com/Life-Experimentalist/dev-prune
                        </a>
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
                        <span className="c-blue">→</span> Caches emptied:
                        {"    "}6.30 GB
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
                        <span className="c-blue">→</span> 2026-08-11 06:00 UTC
                        (2 days ago) — 2.00 GB from 3 directories
                      </div>
                      <div className="term-line">
                        <span className="c-blue">→</span> Put it back with: devp
                        restore --last-run
                      </div>
                      <div className="term-line">&nbsp;</div>
                      <div className="term-line c-bold">Biggest reclaims</div>
                      <div className="term-line">
                        {"     "}4.20 GB ~/Code/MyMonorepo
                        <span className="c-dim">
                          {"   "}(last pruned 2 days ago)
                        </span>
                      </div>
                      <div className="term-line">
                        {"     "}2.90 GB ~/Code/PyDataLab
                        <span className="c-dim">
                          {"   "}(last pruned 12 days ago)
                        </span>
                      </div>
                      <div className="term-line">
                        {"    "}850.0 MB ~/Code/ArchivedApp
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
                        <span className="c-blue">→</span> frontend — pnpm
                        install --frozen-lockfile
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
                        {
                          "  .devprune.json          absent — global settings apply"
                        }
                      </div>
                      <div className="term-line c-dim">
                        {
                          "  Activity                2026-04-11 (31 days ago), threshold 15 — idle"
                        }
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
                        <span className="c-green">✓</span> pnpm-lock.yaml
                        present
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
                        <span className="c-red">✗</span> uv.lock missing —
                        nothing can prove
                      </div>
                      <div className="term-line">
                        {
                          "                          the directory is rebuildable, so it will never"
                        }
                      </div>
                      <div className="term-line">
                        {"                          be pruned"}
                      </div>
                      <div className="term-line">
                        {"    Bloat               "}
                        <span className="c-green">✓</span> services/api/.venv
                        (218.44 MiB)
                      </div>
                      <div className="term-line">&nbsp;</div>
                      <div className="term-line c-dim">Verdict</div>
                      <div className="term-line">
                        {"  "}
                        <span className="c-green">✓</span> Would `devp run`
                        prune this? Yes — frontend has verifiable bloat.
                      </div>
                      <div className="term-line">&nbsp;</div>
                      <div className="term-line">
                        {"  "}
                        <span className="c-red">✗</span> services/api: uv.lock
                        missing …
                      </div>
                      <div className="term-line">&nbsp;</div>
                      <div className="term-line c-dim">
                        {
                          "  Troubleshooting: .../docs/troubleshooting/README.md"
                        }
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
                      <div className="term-line c-dim">
                        {" "}
                        Nothing was changed.
                      </div>
                    </div>
                  )}
                </div>
              </div>
            </div>
          </div>

          {/* The setup prompt and the headline numbers are full-width: in the
              left column they stretched it past the ledger, and on a tall,
              narrow screen the three-up stats grid broke to 2 + 1. */}
          <div className="container hero-tail">
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
                <strong>23</strong>
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
                  every Git repository it finds, or{" "}
                  <code>devp init --auto</code> works out where to look on its
                  own. Git hooks then keep the list current as you clone new
                  ones.
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
                  <code>.devprune.json</code> and the committed{" "}
                  <code>project.devprune.json</code> hold inert data only: an
                  ignore flag, an idle-day override, a display name, automation
                  opt-outs. Neither can name a command, so a repository you have
                  just cloned cannot run anything on your machine.
                </p>
              </div>
              <div className="f-card">
                <div className="f-icon">
                  <ShieldCheck className="c-green" />
                </div>
                <h3>Guess when it cannot read your config</h3>
                <p>
                  A repository config that will not parse skips the repository
                  and reports the syntax error. The unreadable file may have
                  been the one saying <code>"ignore": true</code>.
                </p>
              </div>
              <div className="f-card">
                <div className="f-icon">
                  <EyeOff className="c-blue" />
                </div>
                <h3>Phone home</h3>
                <p>
                  No analytics, no diagnostics, no usage data. One request
                  exists — an unauthenticated release check against GitHub, with
                  no body and no identifier — and{" "}
                  <code>devp config set update_check false</code> ends it. When
                  that check finds a newer release, dev-prune installs it after
                  the next pass — verified against its published SHA-256, and
                  never by handing your machine to a package manager.{" "}
                  <code>devp config set auto_update false</code> leaves
                  upgrading to you, and{" "}
                  <code>devp config set version_lock true</code> pins this copy
                  where it is — nothing dev-prune does replaces the binary after
                  that, not even a re-run of the installer.
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
                Twenty-four managers.{" "}
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
                <Puzzle size={18} /> Twenty-five would be better than
                twenty-four
              </h3>
              <p>
                Adding a manager is deliberately small: implement one{" "}
                <code>PackageManager</code> trait — detect, list bloat
                directories, verify the lockfile, restore — register it in one
                array, and add its fixtures to the adapter test suite. Nothing
                else in the codebase has to know it exists. The obvious
                ecosystems are covered; what is left is the awkward one. Nix
                would have to reason about the store, which is a different kind
                of problem from &ldquo;a lockfile says this comes back&rdquo;.
                Beyond that, name the manager and the recipe is written down.
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
                this repository and nothing else. Worth saying out loud because{" "}
                <code>.</code> is usually treated as a shell detail rather than
                an argument: here it is a real path, it works on every platform,
                and it works the same in every command that takes one.
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
                      setup pass. <code>--auto</code> works out the paths itself
                      instead of being told them
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
                      <code>--top N</code> lists only the N repositories with
                      the most reclaimable space — the totals above the table
                      still cover every one of them. Once a restore has been
                      timed on this machine, the header also estimates how long
                      putting it all back would take
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
                      <code>devp history</code>
                    </td>
                    <td>
                      Which pass deleted what, and what asked it to &mdash; one
                      line per pass, then <code>--pass 1</code> for the exact
                      command line and every directory it took.{" "}
                      <code>--export</code> writes the lot to your documents
                      folder
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
                      cargo to conda, Maven, Gradle, NuGet, vcpkg, Conan,
                      Composer, CocoaPods and Hex — and print the command that
                      clears each. Reports only — it deletes nothing
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp caches --volume V:</code>
                    </td>
                    <td>
                      Only the caches sitting on one drive. <code>--drive</code>{" "}
                      is the same flag, and it takes a mount point{" "}
                      (<code>/mnt/data</code>, <code>/Volumes/Work</code>) or any
                      path on the drive you mean. The unfiltered report ends with
                      a <code>By drive</code> line splitting the total the same
                      way: on a machine whose projects live on a second disk, the
                      machine-wide total is not the figure that decides anything —
                      the gigabytes on the drive that is full are
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp caches docker</code>
                    </td>
                    <td>
                      What a container engine is holding — images, containers,
                      volumes and build cache, each sized, with how much of it
                      the engine says it could give back — then the prune
                      commands and what each takes with it. Also{" "}
                      <code>podman</code>, <code>nerdctl</code>,{" "}
                      <code>finch</code>, Apple&rsquo;s <code>container</code>,
                      and <code>devp caches containers</code> for every engine
                      at once plus any local Kubernetes clusters. The report
                      deletes nothing; the next row does
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp caches clear &lt;engine&gt;</code>
                    </td>
                    <td>
                      Run the narrow prune commands for you &mdash; build cache,
                      unused images, stopped containers &mdash; after printing
                      them and asking, and count what came back on{" "}
                      <code>devp stats</code>. Never a volume: there is no
                      argument in the table containing the word, and a test
                      fails the build if one appears. Name the engine; no
                      schedule, hook or <code>clear all</code> reaches it
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp caches clear &lt;manager&gt;</code>
                    </td>
                    <td>
                      Empty one cache, or <code>all</code> of them, after
                      listing and sizing what goes. Runs the manager's own clear
                      command and measures what was actually freed.{" "}
                      <code>--over-cap</code> narrows it to the managers that
                      have outgrown their <code>cache_max_gb</code>;{" "}
                      <code>--unused</code> to the ones no registered repository
                      uses. Only ever
                      when you type it — no pass, schedule or hook clears a
                      cache, and Maven&rsquo;s <code>~/.m2/repository</code> is
                      never cleared at all
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp trust</code>
                    </td>
                    <td>
                      What dev-prune is allowed to do on this machine, on one
                      screen: what the code guarantees everywhere, then every
                      setting you have switched on that widens it, by name, then
                      every copy of it on the machine &mdash; not just the
                      managed ones: everything on your <code>PATH</code> and in
                      each package manager&rsquo;s install directory, with the
                      manager that put it there and the SHA-256 an antivirus
                      actually sees &mdash; the one on your disk, not the one on
                      the release page
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
                      Put back exactly what the last prune pass deleted, in
                      every repository it touched
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp doctor [PATH]</code>
                    </td>
                    <td>
                      Check the installation, or one repository — ending with
                      the single reason a pass would or would not touch it
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
                      manager icons. <code>devp config wizard</code> opens every
                      setting in a full-screen configurator, including which
                      adapters to switch off;{" "}
                      <code>devp config recommended</code> applies the
                      recommended ones in one command
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
                      agent's skills directory;{" "}
                      <code>--agent &lt;editor&gt;</code> writes per-repository
                      rules for any of sixteen editors
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp update [--install]</code>
                    </td>
                    <td>
                      Print the installed version and the upgrade command;{" "}
                      <code>--install</code> downloads the release binary,
                      verifies its checksum and replaces every copy this install
                      runs
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp install [--channel]</code>
                    </td>
                    <td>
                      Move this install to another package manager — installs
                      through the one you name, then removes the old copy
                      through the one that put it there; <code>--dry-run</code>{" "}
                      prints the plan and runs none of it
                    </td>
                  </tr>
                  <tr>
                    <td className="td-name">
                      <code>devp man [command]</code>
                    </td>
                    <td>
                      The contents page: every command grouped by what it is
                      for. <code>devp man run</code> for one page, roff when
                      redirected, <code>--dir</code> for the full set
                    </td>
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
                  <code>--min-size</code> sets a size floor,{" "}
                  <code>--explain</code> lists every repository with the reason
                  it would or would not be pruned (read-only), and{" "}
                  <code>--json</code> emits one machine-readable document
                  instead of the report, on <code>run</code>,{" "}
                  <code>status</code>, <code>stats</code>, <code>caches</code>{" "}
                  and <code>trust</code>.
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
                <p>
                  The setup pass registers no repository itself. The scheduled
                  prune pass is the one place that can, and only while{" "}
                  <code>auto_discover</code> is on: it looks for repositories you
                  never added, using the same roots as{" "}
                  <code>devp init --auto</code>. A manual <code>devp run</code>{" "}
                  never does it, a repository holding an{" "}
                  <code>ignore.devprune.json</code> is never registered, and
                  being registered still only means being considered — not
                  pruned.
                </p>
                <p className="muted">
                  Off switches: <code>auto_daemon</code>,{" "}
                  <code>auto_discover</code>, <code>auto_hooks</code>,{" "}
                  <code>auto_setup</code>, or{" "}
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
                  <code>.devprune.json</code> is yours: dev-prune writes it into{" "}
                  <code>.git/info/exclude</code>, so it never reaches a commit.{" "}
                  <code>devp config project . --team</code> writes the same
                  keys to <code>project.devprune.json</code>, which is meant to be
                  committed — every key it names wins, and your file answers
                  the rest. Or drop an empty <code>ignore.devprune.json</code> in
                  the root to opt out entirely. All three are read from the
                  repository root and nowhere else, because every path inside
                  them is relative to that root — a copy one directory down
                  parses, looks applied and is read by nothing.{" "}
                  <code>devp doctor</code> names any it finds, and leaves them
                  where they are: moving one up a level would silently change
                  what every path inside it means.
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
                  <Bot size={18} /> Rules for your editor
                </h3>
                <div className="cmd-line small">
                  <span className="cmd-prompt">$</span>
                  <code className="cmd-code">devp skill --agent antigravity</code>
                  <CopyButton
                    id="skill-agent"
                    text="devp skill --agent antigravity"
                    copied={copiedKey}
                    onCopy={handleCopy}
                  />
                </div>
                <p className="muted">
                  Writes the per-repository rules file that editor actually
                  reads — <code>.agent/rules/</code> for Gemini Antigravity,
                  <code>.cursor/rules/</code> for Cursor, and so on. Sixteen
                  targets: Antigravity, Cursor, Windsurf, Cline, Roo, Kilo Code,
                  Continue, Amazon Q, Kiro and Trae get their own file; Copilot,
                  Gemini CLI, Junie, Zed, Aider and <code>AGENTS.md</code> get a
                  marked block inside a file they already share. Aider is the one
                  that has to be told to read its{" "}
                  <code>CONVENTIONS.md</code> — the command says so after
                  writing it. Run <code>devp skill --help</code> for the exact
                  paths.
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

        {/* ------------------------------ editors ------------------------------ */}
        <Reveal as="section" className="section" id="editors">
          <div className="container">
            <div className="section-header">
              <h2 className="section-title">
                And in <span className="gradient-text">your editor</span>
              </h2>
              <p className="section-subtitle">
                Nothing here is required — the CLI is the product. But the
                config file is easier to write when the editor knows its shape,
                and a repository's reclaimable size is easier to notice when it
                is already on screen.
              </p>
            </div>

            <div className="split-grid">
              <div className="info-card">
                <h3>
                  <Puzzle size={18} /> The extension
                </h3>
                <div className="cmd-line small">
                  <span className="cmd-prompt">$</span>
                  <code className="cmd-code">
                    code --install-extension VKrishna04.dev-prune
                  </code>
                  <CopyButton
                    id="ext"
                    text="code --install-extension VKrishna04.dev-prune"
                    copied={copiedKey}
                    onCopy={handleCopy}
                  />
                </div>
                <p className="muted">
                  Validates <code>.devprune.json</code> as you type — every key,
                  every adapter name, every enum — from the schema bundled
                  inside it rather than fetched, so it works offline. The
                  workspace's reclaimable size sits in the status bar.
                </p>
                <p className="muted">
                  <a href={MARKETPLACE} target="_blank" rel="noreferrer">
                    VS Code Marketplace <ExternalLink size={13} />
                  </a>{" "}
                  ·{" "}
                  <a href={OPENVSX} target="_blank" rel="noreferrer">
                    Open VSX <ExternalLink size={13} />
                  </a>{" "}
                  — the same extension for VSCodium, Cursor, Windsurf, Positron
                  and Kiro, which cannot reach Microsoft's registry.
                </p>
              </div>
              <div className="info-card">
                <h3>
                  <FileCode size={18} /> No extension needed
                </h3>
                <p>
                  The config schema is registered with{" "}
                  <a
                    href="https://www.schemastore.org/"
                    target="_blank"
                    rel="noreferrer"
                  >
                    SchemaStore
                  </a>
                  , so IntelliJ, PyCharm, WebStorm, GoLand, RubyMine, Rider,
                  Visual Studio, Neovim and Zed validate{" "}
                  <code>.devprune.json</code> out of the box. There is nothing
                  to install and nothing to configure.
                </p>
                <p className="muted">
                  <code>devp setup</code> offers the extension once, at a
                  terminal, into whichever editors it finds — each from its own
                  registry.
                </p>
              </div>
              <div className="info-card">
                <h3>
                  <Bot size={18} /> And your coding agent
                </h3>
                <div className="cmd-line small">
                  <span className="cmd-prompt">$</span>
                  <code className="cmd-code">devp skill --agent cursor</code>
                  <CopyButton
                    id="skill-agent"
                    text="devp skill --agent cursor"
                    copied={copiedKey}
                    onCopy={handleCopy}
                  />
                </div>
                <p className="muted">
                  Writes the rules file that editor actually reads —{" "}
                  <code>.github/copilot-instructions.md</code>,{" "}
                  <code>.cursor/rules/</code>, <code>CLAUDE.md</code>,{" "}
                  <code>.junie/guidelines.md</code> and the rest — so an agent
                  working in the repository knows what dev-prune will and will
                  not delete before it suggests anything.
                </p>
                <p className="muted">
                  <a
                    href={`${DOCS}/IDE_INTEGRATION.md`}
                    target="_blank"
                    rel="noreferrer"
                  >
                    Everything about editors, in one page{" "}
                    <ExternalLink size={13} />
                  </a>
                </p>
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
              <Faq q="Is there a --force flag?">
                Yes, and it still works — but not for what the name suggests. It
                is the deprecated spelling of <code>--ignore-idle</code>, which
                is all it has ever done: lift the idle-day wait, and nothing
                else. It was renamed because “force” reads like “override the
                safety checks”, and there is no flag that does that. Typing{" "}
                <code>--force</code> prints a one-line note pointing at the new
                name, along with the seven reasons a directory gets skipped and
                how to fix each one.
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
                one running dev-prune's registration and then <code>exec</code>
                -ing the original — so husky still fires, and{" "}
                <code>devp hook uninstall</code> puts the old path back exactly
                as it was. If the other tool later adds a hook, the next setup
                pass notices the drift and rebuilds the shims.
              </Faq>
              <Faq q="How do I stop it installing background things?">
                It already stops itself in the places that would be wrong: a CI
                runner, a container, or any non-interactive session is detected
                and the pass is skipped without being asked. Otherwise{" "}
                <code>devp config set auto_setup false</code> turns off the
                whole pass, <code>auto_hooks</code>, <code>auto_daemon</code> and{" "}
                <code>auto_discover</code> turn off one part each, and{" "}
                <code>DEV_PRUNE_NO_AUTO_SETUP=1</code> overrides all of them
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
              <Faq q="Does it delete build output — dist/, .next/, target/?">
                Not by default, and never on a rule of its own. A prune only
                removes a directory something can prove comes back, and a build
                output has no lockfile: nothing can promise <code>dist/</code>{" "}
                rebuilds byte-for-byte, so there is no <code>dist/</code> rule
                and there will not be one. A repository can still declare it —{" "}
                <code>prunable.directories</code> names the path and the{" "}
                <code>rebuild</code> command that puts it back, which dev-prune
                checks is installed before it deletes. That file is committed,
                so <code>prunable.exclude</code> is the other half: it takes a
                declared path back out on the one machine that is keeping it,
                without editing a file the whole team shares. Across the two
                files that is the whole point; naming one path in both lists of
                the <em>same</em> file is a typo rather than a decision, because
                the exclusion still wins and the declaration then never runs
                — <code>devp doctor</code> says so rather than letting it pass
                quietly. The
                eight exceptions are opt-in and say so —{" "}
                <code>devp config set enable_cargo true</code> (
                <code>target/</code>), <code>enable_gradle</code> (
                <code>build/</code>, <code>.gradle/</code>),{" "}
                <code>enable_maven</code> (<code>target/</code>),{" "}
                <code>enable_swift</code> (<code>.build/</code>),{" "}
                <code>enable_dart</code> (<code>.dart_tool/</code>),{" "}
                <code>enable_mix_build</code> (<code>_build/</code>),{" "}
                <code>enable_vcpkg</code> (<code>vcpkg_installed/</code>) and{" "}
                <code>enable_cmake_build</code> (a tree holding a{" "}
                <code>CMakeCache.txt</code> that names sources in this
                repository) — whose
                claim is rebuild-from-source rather than
                reinstall-from-lockfile, which is why they ship off and wait an
                extra <code>build_idle_days</code> (45) before they touch
                anything.
              </Faq>
              <Faq q="Can I turn one ecosystem off for good?">
                <code>devp config set disabled_adapters go,composer</code> makes
                dev-prune behave as though those two were not installed at all:
                not detected, not counted, not probed for by doctor, never
                pruned and never restored.{" "}
                <code>devp config set disabled_adapters -</code> turns them back
                on, and <code>devp config wizard</code> offers the same list as
                a checklist. For a single pass,{" "}
                <code>devp run --only npm,cargo</code> and{" "}
                <code>--skip go</code> do the same thing temporarily. Every
                setting that <em>widens</em> what is deletable is listed by name
                in <code>devp trust</code> — no letter grade, just which
                switches are on and how to put each one back.
              </Faq>
              <Faq q="Does it touch anything outside my repositories?">
                A prune never does — it stays inside registered repositories and
                never crosses a <code>.git</code> boundary. The machine-wide
                package caches (<code>~/.npm</code>, <code>~/.cargo</code>,{" "}
                <code>~/.m2</code>, <code>~/.gradle</code> and the rest) are a
                separate, explicit command: <code>devp caches</code> reports
                them and only <code>devp caches clear &lt;manager&gt;</code>{" "}
                removes one, after confirming. Maven&rsquo;s{" "}
                <code>~/.m2/repository</code> is the one it will not remove even
                then: <code>mvn install:install-file</code> puts artifacts there
                that exist in no remote, so dev-prune sizes it, prints the
                command and lets you decide.
              </Faq>
              <Faq q="Docker is bigger than all of these put together. Does it help?">
                It is usually the biggest single thing on a developer's disk,
                and since 1.17.0 the same binary clears it.{" "}
                <code>devp caches docker</code> — or <code>podman</code>,{" "}
                <code>nerdctl</code>, <code>finch</code>, Apple&rsquo;s{" "}
                <code>container</code>, or <code>devp caches containers</code>{" "}
                for all of them — breaks the space down into images, containers,
                local volumes and build cache, says how much of each the engine
                itself calls reclaimable, and then prints the prune commands
                narrowest first with what each one takes with it. Then{" "}
                <code>devp caches clear docker</code> runs the narrow ones for
                you: build cache, unused images, stopped containers, printed
                first, after a prompt, and counted on <code>devp stats</code>{" "}
                so the space you reclaimed on its advice is space it can account
                for. It will not touch a volume, and there is no flag that makes
                it &mdash; an image can be pulled again and a build cache
                rebuilt, but what is inside a named volume is the only copy, so{" "}
                <code>docker volume prune</code> stays a command it prints and
                you type. Nothing on a schedule goes near any of it. The figures
                come from the engine's own{" "}
                <code>system df</code> rather than a look at the disk, because on
                Docker Desktop and Podman the store lives inside a VM disk image
                your filesystem cannot see — and because asking is the only way
                to learn what is <em>reclaimable</em>, which is the number that
                decides anything. If the daemon is stopped, you get that sentence
                and no figures: a blank, not a zero.
              </Faq>
              <Faq q="Which of these caches does anything still need?">
                <code>devp caches</code> answers it per manager: how many of
                your registered repositories use it, and what its cache works
                out to per repository. Two repositories sharing a 12&nbsp;GiB
                cache is 6&nbsp;GiB each and worth a look; forty sharing the
                same 12&nbsp;GiB is 300&nbsp;MiB each and is the cache doing its
                job. A manager <em>no</em> registered repository uses is the one
                case where a count is enough to act on — everything in it was
                downloaded for projects that are not on this disk any more — and{" "}
                <code>devp caches clear --unused all</code> empties exactly
                those. The count ignores whether an adapter is switched on,
                because the question is which managers your projects use, not
                which ones a prune pass would touch. It is shown only for the
                managers that are also adapter names;{" "}
                <code>pip</code>, <code>conda</code>, <code>nuget</code>,{" "}
                <code>conan</code> and <code>hex</code> get no number rather than
                a guess. With nothing registered, nothing is counted and{" "}
                <code>--unused</code> refuses to run — every cache would look
                unused.
              </Faq>
              <Faq q="My projects are on a second drive. Did it find the right pnpm store?">
                Yes, and that is why this row exists at all. pnpm hardlinks its
                store into every <code>node_modules</code> it fills, and a
                hardlink cannot cross a filesystem — so projects kept off the
                system disk get a store of their own at the root of{" "}
                <em>their</em> filesystem: <code>V:\.pnpm-store</code> on a
                second Windows drive, <code>/mnt/data/.pnpm-store</code> on
                Linux, <code>/Volumes/Work/.pnpm-store</code> on an external
                macOS volume. It is not a Windows idea; it is wherever a
                developer keeps projects off the system disk.{" "}
                <code>pnpm store path</code> only ever answers for the
                filesystem it is run on, so a machine-wide report asked from
                your home directory finds the small store beside it and misses
                the multi-gigabyte one holding your actual projects.{" "}
                <code>devp caches</code> looks at the root of every filesystem
                that holds a registered repository, plus the one you are
                standing in, and gives each store a row that names itself in its
                clear command:{" "}
                <code>pnpm store prune --store-dir &lt;path&gt;</code>.
              </Faq>
              <Faq q="My uv cache is over 10 GB. Can it tell me?">
                <code>devp config set cache_max_gb uv=10,npm=10</code> writes
                down how big is too big, per manager, in gibibytes &mdash;
                GiB, the unit the report prints. A cache is a
                bet that re-downloading costs more than the disk it occupies,
                and somewhere the bet stops paying — this is where you say
                where. A manager past its ceiling is marked in{" "}
                <code>devp caches</code>, measured against its whole footprint,
                so cargo&rsquo;s registry cache and its unpacked sources are
                weighed together. <strong>Setting a cap deletes nothing.</strong>{" "}
                It marks, and <code>devp caches clear --over-cap all</code>{" "}
                empties exactly what is marked, when you type it. Empty by
                default: no cache is too big until you say what too big is. The
                keys are the names <code>devp caches clear</code> takes —{" "}
                <code>npm</code>, <code>pnpm</code>, <code>uv</code>,{" "}
                <code>pip</code>, <code>cargo</code>, <code>go</code>,{" "}
                <code>nuget</code> and the rest — and{" "}
                <code>devp config wizard</code> sets them as a column beside the
                adapter checklist.
              </Faq>
              <Faq q="Can I install it with npm?">
                Yes — <code>npm install -g dev-prune</code>, or{" "}
                <code>npx dev-prune status</code> to run it once without
                installing anything. It works the way esbuild and Biome do: a
                small <code>dev-prune</code> package lists seven platform
                packages as optional dependencies, and npm installs only the one
                matching your machine. The binary lives inside that package, so
                there is no download step at install time — it installs under{" "}
                <code>npm ci --ignore-scripts</code>, behind a registry mirror,
                and with no access to GitHub. Windows needs 1.8.0 or later: the
                three Windows packages did not exist before then, and a
                published manifest cannot be edited, so a machine still holding{" "}
                <code>dev-prune@1.7.0</code> needs{" "}
                <code>npm install -g dev-prune@latest</code> rather than a
                repair. bun, pnpm and Yarn install that same package —{" "}
                <code>bun add -g dev-prune</code>,{" "}
                <code>pnpm add -g dev-prune</code>,{" "}
                <code>yarn global add dev-prune</code> — and dev-prune treats
                each as a channel of its own rather than as npm, because the four
                do not share records. A copy bun installed is upgraded with bun
                and removed with bun; running npm against it would add a second
                copy under npm&rsquo;s prefix and leave bun&rsquo;s, still on
                PATH, at the old version.{" "}
                <code>devp update --channels</code> prints every
                channel&rsquo;s upgrade command.
              </Faq>
              <Faq q="Which copy of devp am I actually running?">
                <code>devp doctor</code> answers that. Installing over time from
                pip, npm, cargo or uv leaves copies in several places, and the
                one first on PATH is not always the one the scheduler and git
                hooks invoke — doctor reports every copy it finds and the
                version each is, plus an <em>Install receipt</em> line for a copy
                one of the install scripts wrote, naming its version, which
                script wrote it and when. The one-liner asks about a copy it
                finds rather than deleting it: answer <code>y</code> and the
                older binary runs{" "}
                <code>devp install --channel installer</code> itself, installing
                here and uninstalling there through the manager that owns it.{" "}
                <code>devp update --install</code> instead
                upgrades all of them at once: it downloads the release binary
                for your platform, checks it against the SHA-256 published
                beside it, and installs nothing if that does not match.
              </Faq>
              <Faq q="Does it work with Chinese, Japanese or Korean paths?">
                Yes, on all three platforms. Paths are handled as real
                filesystem paths, never as byte strings, so a repository at{" "}
                <code>ワークスペース/项目目录名称测试</code> is scanned, pruned
                and restored exactly like an ASCII one. The terminal output
                measures display <em>columns</em> rather than characters, which
                is what keeps the tables in <code>devp status</code> and{" "}
                <code>devp doctor</code> aligned when a name is full-width — one
                CJK character occupies two columns, and padding it by character
                count is the usual reason a tool's columns go crooked. The same
                holds for accented Latin, Cyrillic, Arabic and emoji directory
                names.
              </Faq>
              <Faq q="How do I remove it?">
                <code>devp uninstall</code> removes the program: the scheduler,
                hooks, the installed agent skill, the PATH entry and the
                binaries themselves — then finds every other copy that pip, npm,
                cargo or uv left behind and removes them all after one
                confirmation, printing each manager's own uninstall line so its
                records clear too. On Windows a running program cannot delete
                itself, so the copy you invoked is renamed aside — the command
                stops resolving straight away — and Windows removes what is left
                at the next restart. Nothing is left running behind it. Add{" "}
                <code>--deep</code> to also wipe the configuration directory and
                every registered repository's <code>.devprune.json</code> — it
                asks first.
              </Faq>
            </div>
          </div>
        </Reveal>

        {/* ----------------------------- Languages ---------------------------- */}
        <Reveal as="section" className="section" id="languages">
          <div className="container narrow">
            <h2 className="section-title">Everywhere developers have disks</h2>
            <p className="section-subtitle">
              Paths in any script prune and restore the same way — and the
              headings above them come in twelve languages, not just this pitch.
            </p>
            <Languages />
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
            <img src="/assets/icon_small.png" alt="" width="24" height="24" />
            <span>dev-prune (devp)</span>
            <span className="footer-tagline">
              · lockfile-safe workspace pruner
            </span>
          </div>
          <nav className="footer-links" aria-label="Footer">
            <a href={REPO} target="_blank" rel="noreferrer">
              GitHub
            </a>
            <a href="/blog/">Guides</a>
            <a href={`${DOCS}/README.md`} target="_blank" rel="noreferrer">
              Docs
            </a>
            <a href="/reference/">CLI reference</a>
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
            <a href={PORTFOLIO} target="_blank" rel="noreferrer">
              vkrishna04.me
            </a>
          </nav>
        </div>
        <div className="container">
          <p className="footer-legal">
            Copyright 2026{" "}
            <a href={PORTFOLIO} target="_blank" rel="noreferrer">
              VKrishna04
            </a>{" "}
            · Licensed under the Apache License, Version 2.0
          </p>
        </div>
      </footer>
    </div>
  );
}
