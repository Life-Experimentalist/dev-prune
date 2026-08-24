# Set up dev-prune with an AI assistant

You don't have to read the install docs. Hand the prompt below to any AI coding
assistant — **Claude Code, Cursor, GitHub Copilot, Windsurf, Gemini / Antigravity**, or
any other agent that can run terminal commands — and it will install `dev-prune`, verify
it, and register the projects you want kept clean.

The prompt is deliberately self-contained: it names the one-liners for every platform,
tells the agent how to pick the right one, and — most importantly — tells it what **not**
to do (no manual scheduler edits, no touching your PATH by hand, no guessing at repos to
register). Everything it needs is already automated by `devp setup`.

---

<p align="center">
  <img src="../assets/github-readme-banner.png" alt="dev-prune — gigabytes back, nothing you can't rebuild" width="800" />
</p>

---

## The prompt

Copy everything inside the box and paste it to your assistant.

````text
Install and set up `dev-prune` (binary name `devp`), a lockfile-safe workspace cleaner,
on this machine. Follow these steps exactly and do not improvise beyond them.

1. Detect the OS and run the matching official installer, nothing else:
   - macOS or Linux:
       curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
   - Windows (PowerShell):
       iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex
   - If a Rust toolchain is already present and you cannot reach the network, you may
     instead run:  cargo install dev-prune
   Do NOT download binaries from anywhere other than devprune.vkrishna04.me or the
   project's GitHub releases, and do NOT edit PATH, the registry, or any OS scheduler by
   hand — the installer and `devp setup` do all of that themselves.

2. Open a NEW terminal (so the updated PATH is in effect) and verify:
       devp --version
       devp doctor
   `devp doctor` must exit 0. If it prints warnings, read them out to me; do not try to
   "fix" the scheduler or hooks yourself — they are self-installing.

3. Ask me which project directories to keep clean, then register each one:
       devp init <path>
   Do not register directories I did not name. `devp init` only records a directory; it
   never deletes anything on its own.

4. Show me the result and stop:
       devp status

Notes you should rely on, not work around:
- Installation already registered a background pass (every 2 days) and, on Windows, a
  windowless task that never flashes a console window. You do not need to configure any
  of this.
- Nothing is ever deleted unless a lockfile can rebuild it, the repo has been idle past
  the threshold, and (interactively) I confirm. Run `devp run --dry-run` if I want a
  preview.
- To undo the whole thing later: `devp uninstall`.
````

---

## What the agent will have done

- Installed the `devp` (and `dev-prune`) binary from the official channel for your OS.
- Let `devp setup` put the background scheduler and Git hooks in place — see
  [Background Automation](BACKGROUND_AUTOMATION.md).
- Registered only the repositories you named, with `devp init`.
- Left every safety guarantee intact: `dev-prune` still refuses to delete anything a
  lockfile cannot rebuild. See [Safety Invariants](SAFETY_INVARIANTS.md).

## Related

- `devp skill` prints the embedded AI Skill and ready-to-copy onboarding prompts for
  agents that support skills — see the
  [AI Pair Programming section](README.md#-ai-pair-programming--agent-integration).
- [CLI Command Reference](CLI_REFERENCE.md) documents every command the prompt uses.
- [Distribution & Packaging Manual](DISTRIBUTION.md) explains each install channel.
