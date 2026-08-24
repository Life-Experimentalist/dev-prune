#!/usr/bin/env node
// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Launcher for the dev-prune native binary.
//
// dev-prune is a Rust program, so this package ships no JavaScript implementation. The
// binary arrives through `optionalDependencies`: one package per platform, each marked
// with `os` and `cpu`, so npm installs exactly the one that matches and silently skips
// the other five. That is the same mechanism esbuild, biome and swc use, and it means
// `npx dev-prune` works with nothing installed beforehand.
//
// If that package is not there — `--no-optional`, an unsupported platform, or an
// install that predates this layout — fall back to a binary put on the machine by
// `cargo install dev-prune` or by the platform installer script. Deliberately no
// repo-relative `target/release` lookup: that made `npx dev-prune` execute whatever
// binary happened to sit in a nearby build directory.
const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');

const isWin = process.platform === 'win32';
const exeName = isWin ? 'dev-prune.exe' : 'dev-prune';

// npm refused every `dev-prune-win32-*` name with `E403 - Package name triggered spam
// detection`, while the identically-shaped linux and darwin names went through, so the
// refusal tracks the name rather than the payload. The Windows packages are named after
// the release assets instead. Each one still declares npm's own `win32`/`ia32` values in
// `os` and `cpu`, so resolution is unchanged - only the name it resolves to differs.
const WINDOWS_ARCH = { x64: 'x64', arm64: 'arm64', ia32: 'x86' };
const platformPackage = isWin
  ? `dev-prune-windows-${WINDOWS_ARCH[process.arch] || process.arch}`
  : `dev-prune-${process.platform}-${process.arch}`;

// Must mirror Registry::config_dir() in src/config.rs.
function configBinDir() {
  if (process.env.DEV_PRUNE_CONFIG_DIR) {
    return path.join(process.env.DEV_PRUNE_CONFIG_DIR, 'bin');
  }
  if (isWin) {
    const appData = process.env.APPDATA || path.join(os.homedir(), 'AppData', 'Roaming');
    return path.join(appData, 'dev-prune', 'bin');
  }
  if (process.platform === 'darwin') {
    return path.join(os.homedir(), 'Library', 'Application Support', 'dev-prune', 'bin');
  }
  const xdg = process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config');
  return path.join(xdg, 'dev-prune', 'bin');
}

/// The binary shipped in this install's platform package, or null if it is absent.
function bundledBinary() {
  try {
    return require.resolve(`${platformPackage}/bin/${exeName}`);
  } catch (_) {
    return null;
  }
}

const fallbacks = [
  path.join(configBinDir(), exeName),
  path.join(os.homedir(), '.cargo', 'bin', exeName),
];

const binaryPath =
  bundledBinary() || fallbacks.find((c) => fs.existsSync(c)) || exeName;

const child = spawn(binaryPath, process.argv.slice(2), { stdio: 'inherit' });

// Forward termination to the child. Ctrl+C already reaches it through the shared
// process group, but a bare `kill` of this wrapper — process managers, CI timeouts —
// otherwise orphans the binary mid-prune while the wrapper reports itself gone.
for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  try {
    process.on(sig, () => {
      try {
        child.kill(sig);
      } catch (_) {}
    });
  } catch (_) {
    // Windows only supports some of these; an unregistrable one stays unforwarded.
  }
}

// Without this, a missing binary surfaces as an unhandled 'error' event and a Node
// stack trace instead of an actionable message.
child.on('error', (err) => {
  if (err.code === 'ENOENT' && binaryPath !== exeName && fs.existsSync(binaryPath)) {
    // The file is right there, yet the loader said "no such file" — the kernel's
    // report for a binary it cannot load, typically one built for a different
    // architecture. "Install what you already installed" would loop the user.
    console.error(
      `dev-prune: '${binaryPath}' exists but could not be executed.\n\n` +
        `It may be built for a different architecture than ${process.platform}-${process.arch},\n` +
        'or the file is damaged. Reinstall, or build a native binary:\n' +
        '  npm install --force dev-prune\n' +
        '  cargo install dev-prune'
    );
  } else if (err.code === 'ENOENT') {
    console.error(
      `dev-prune: could not find the '${exeName}' binary.\n\n` +
        `No prebuilt binary is published for ${process.platform}-${process.arch}, or the\n` +
        `'${platformPackage}' package was skipped (npm --no-optional does that).\n\n` +
        'Install it with one of:\n' +
        '  npm install --include=optional dev-prune\n' +
        '  cargo install dev-prune\n' +
        '  curl -fsSL https://devprune.vkrishna04.me/install.sh | sh\n\n' +
        'Looked in:\n' +
        [`  ${platformPackage} (optional dependency)`]
          .concat(fallbacks.map((c) => `  ${c}`))
          .join('\n') +
        '\n  (and PATH)'
    );
  } else {
    console.error(`dev-prune: failed to launch ${binaryPath}: ${err.message}`);
  }
  process.exit(127);
});

// `code` is null when the child was killed by a signal; `code || 0` reported success
// in that case. Mirror the shell convention of 128 + signal number instead.
child.on('exit', (code, signal) => {
  if (signal) {
    process.exit(128 + (os.constants.signals[signal] || 0));
  }
  process.exit(code === null ? 1 : code);
});
