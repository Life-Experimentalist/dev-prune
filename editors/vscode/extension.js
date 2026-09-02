// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// The schema mapping in package.json needs no code. This file adds the parts
// that do: a status bar item that walks the workspace through dev-prune's
// lifecycle (CLI missing → not a git repo → not registered → active →
// candidate → cleaned), a QuickPick of actions behind it, palette commands,
// and two notices — one when .devprune.json exists but devp is not on PATH,
// one offering the AI agent skill. Anything that deletes stays in the
// terminal, spelled out where the user can read it before running it.

const { execFile } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const vscode = require('vscode');

const HIDE_KEY = 'devprune.hideCliMissingNotice';
const HIDE_SKILL_KEY = 'devprune.hideSkillOfferNotice';
const INSTALL_URL = 'https://devprune.vkrishna04.me';
const DOCS_URL = 'https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/CLI_REFERENCE.md';
const SCHEMA_URL = 'https://devprune.vkrishna04.me/schemas/v1/devprune.schema.json';

const BIN = process.platform === 'win32' ? 'devp.exe' : 'devp';
// cwd is pinned outside the workspace so the lookup can never resolve
// through workspace content on platforms that search cwd first. The timeout
// budgets for `devp status` walking every registered repository: ~8s measured
// over 47 of them, times whatever an antivirus scan or a cold disk cache adds.
const EXEC_OPTS = { cwd: os.homedir(), timeout: 60000 };

// Where `devp setup` puts its managed copy, per platform. VS Code keeps the
// PATH it was launched with, so a CLI installed a minute ago is invisible to
// PATH lookup until the whole app restarts — probing these directories finds
// a fresh install without any reload.
const KNOWN_BIN_DIRS = (() => {
	const home = os.homedir();
	if (process.platform === 'win32') {
		const appData = process.env.APPDATA || path.join(home, 'AppData', 'Roaming');
		return [path.join(appData, 'dev-prune', 'bin')];
	}
	if (process.platform === 'darwin') {
		return [
			path.join(home, 'Library', 'Application Support', 'dev-prune', 'bin'),
			path.join(home, '.local', 'bin'),
		];
	}
	return [path.join(home, '.config', 'dev-prune', 'bin'), path.join(home, '.local', 'bin')];
})();

function probeKnownLocations() {
	for (const dir of KNOWN_BIN_DIRS) {
		const candidate = path.join(dir, BIN);
		try {
			if (fs.existsSync(candidate)) return candidate;
		} catch {
			// Unreadable directory — keep looking.
		}
	}
	return undefined;
}

function notificationsEnabled() {
	return vscode.workspace.getConfiguration('devprune').get('notifications', true);
}

let statusItem;
// The lifecycle state the last refresh landed in; the QuickPick offers the
// actions that make sense *for that state*, so the two must come from the same
// pass. One of: 'cli-missing' | 'no-git' | 'unlinked' | 'active' | 'candidate'
// | 'cleaned' | 'other'.
let currentState = 'other';
// The workspace repo's entry from the last `status --json`, when one matched.
let currentEntry;
// Starts as the bare name (PATH lookup); switches to an absolute path when a
// probe of the managed install directories finds the CLI there instead.
let binPath = BIN;
// One popup per workspace per session; the status bar carries the state after.
let noticeShownThisSession = false;

function formatBytes(bytes) {
	if (!Number.isFinite(bytes) || bytes < 0) return '0 B';
	const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
	let value = bytes;
	let unit = 0;
	while (value >= 1024 && unit < units.length - 1) {
		value /= 1024;
		unit += 1;
	}
	return `${value >= 10 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

function workspaceRoot() {
	const folders = vscode.workspace.workspaceFolders;
	return folders && folders.length > 0 ? folders[0].uri.fsPath : undefined;
}

// Compare filesystem paths the way the platform does: Windows paths are
// case-insensitive and arrive with either slash from the CLI's JSON.
function normalizePath(p) {
	let out = p.replace(/[\\/]+$/, '').replace(/\\/g, '/');
	if (process.platform === 'win32') out = out.toLowerCase();
	return out;
}

// The registry entry governing this workspace: an exact match, or the deepest
// registered ancestor (a workspace opened on a subfolder of a registered repo
// is still that repo's).
function findRepoEntry(doc, root) {
	const target = normalizePath(root);
	let best;
	let bestLen = -1;
	for (const repo of doc.repositories || []) {
		const p = normalizePath(repo.path || '');
		if ((target === p || target.startsWith(`${p}/`)) && p.length > bestLen) {
			best = repo;
			bestLen = p.length;
		}
	}
	return best;
}

function isGitRepo(root) {
	try {
		// `.git` is a directory in a normal clone and a file in a worktree;
		// either means Git owns this folder.
		return fs.existsSync(path.join(root, '.git'));
	} catch {
		return false;
	}
}

// The suffix explaining why a pnpm/bun repo's reclaimable number looks small:
// the store hardlinks most of the bytes, and deleting the folder cannot free
// what the store still holds.
function sharedBytesNote(entry) {
	let shared = 0;
	const adapters = new Set();
	for (const dir of entry.directories || []) {
		if (dir.shared_bytes > 0) {
			shared += dir.shared_bytes;
			adapters.add(dir.name);
		}
	}
	if (shared === 0) return '';
	return (
		`\n\n**Why so low?** ${formatBytes(shared)} more is hardlinked into the ` +
		`package-manager store (pnpm/bun), so deleting the folder cannot free it — ` +
		`the store keeps those bytes either way.`
	);
}

function showCliMissing(context) {
	currentState = 'cli-missing';
	currentEntry = undefined;
	const root = workspaceRoot();
	// The activation globs also match ignore.devprune.json — the opt-out
	// marker. Only nag when there is something for devp to act on: a real
	// config file, or a Git repository the user could register.
	vscode.workspace.findFiles('**/.devprune.json', undefined, 1).then((found) => {
		const hasConfig = found && found.length > 0;
		if (!hasConfig && !(root && isGitRepo(root))) {
			statusItem.hide();
			return;
		}
		statusItem.text = '$(devprune-logo) devp not found';
		statusItem.tooltip = hasConfig
			? 'This workspace has a .devprune.json, but the dev-prune CLI (devp) is not on your PATH — nothing is acting on that file.'
			: 'The dev-prune CLI (devp) is not installed. It reclaims disk space from idle repositories — click to install it.';
		statusItem.backgroundColor = new vscode.ThemeColor('statusBarItem.warningBackground');
		statusItem.show();
		if (
			!hasConfig ||
			noticeShownThisSession ||
			context.globalState.get(HIDE_KEY) ||
			!notificationsEnabled()
		) {
			return;
		}
		noticeShownThisSession = true;
		// This popup exists to get the CLI installed, nothing more. Everything
		// else lives in the status bar.
		vscode.window
			.showInformationMessage(
				'The dev-prune CLI (devp) is not installed, so nothing is acting on this workspace’s .devprune.json.',
				'Install devp',
				'I installed it',
				"Don't show again",
			)
			.then((choice) => {
				if (choice === 'Install devp') {
					vscode.env.openExternal(vscode.Uri.parse(INSTALL_URL));
				} else if (choice === 'I installed it') {
					recheckCli(context);
				} else if (choice === "Don't show again") {
					context.globalState.update(HIDE_KEY, true);
				}
			});
	});
}

// Re-probe after the user says they installed the CLI. The probe covers the
// managed install directories, so a stale PATH usually doesn't matter — but
// when it does, offer the reload rather than leaving a warning that never
// clears.
function recheckCli(context) {
	refreshStatus(context, () => {
		vscode.window
			.showWarningMessage(
				'devp still not found. VS Code keeps the PATH it was launched with — reloading the window usually picks the new install up.',
				'Reload window',
			)
			.then((choice) => {
				if (choice === 'Reload window') {
					vscode.commands.executeCommand('workbench.action.reloadWindow');
				}
			});
	});
}

// Render one lifecycle state onto the status bar item. Order of precedence:
// CLI missing → not a git repo → not registered → candidate → cleaned →
// active → everything else. Only the CLI-missing state colours the
// background — the API offers warning/error backgrounds only, and a repo
// full of reclaimable bytes is not a warning.
function renderState(doc, root) {
	statusItem.backgroundColor = undefined;

	if (!isGitRepo(root)) {
		currentState = 'no-git';
		currentEntry = undefined;
		statusItem.text = '$(devprune-logo) devp: no git repo';
		statusItem.tooltip = new vscode.MarkdownString(
			'**dev-prune** works per Git repository, and this folder is not one yet.\n\n' +
				'Click to run `git init` — everything else follows from there.',
		);
		statusItem.show();
		return;
	}

	const entry = findRepoEntry(doc, root);
	if (!entry) {
		currentState = 'unlinked';
		currentEntry = undefined;
		statusItem.text = '$(devprune-logo) devp: not linked';
		statusItem.tooltip = new vscode.MarkdownString(
			'**dev-prune** does not have this repository registered.\n\n' +
				'Click to run `devp link .` so its dependency folders are tracked and pruned when idle.',
		);
		statusItem.show();
		return;
	}
	currentEntry = entry;

	const managers =
		entry.adapters && entry.adapters.length > 0 ? entry.adapters.join(', ') : 'none detected';
	const occupied = entry.reclaimable_bytes || 0;

	if (entry.state === 'candidate') {
		currentState = 'candidate';
		statusItem.text = `$(devprune-logo) ${formatBytes(occupied)} reclaimable`;
		statusItem.tooltip = new vscode.MarkdownString(
			`**dev-prune** — this repository is idle: ${formatBytes(occupied)} is ready to prune.\n\n` +
				`Package managers in use: ${managers}.` +
				sharedBytesNote(entry) +
				'\n\nClick for actions.',
		);
		statusItem.show();
		return;
	}

	if ((entry.bytes_freed || 0) > 0 && occupied === 0) {
		currentState = 'cleaned';
		statusItem.text = `$(devprune-logo) devp saved ${formatBytes(entry.bytes_freed)}`;
		const when = entry.last_pruned_at ? ` (last pruned ${entry.last_pruned_at.slice(0, 10)})` : '';
		statusItem.tooltip = new vscode.MarkdownString(
			`**dev-prune** has freed ${formatBytes(entry.bytes_freed)} from this repository${when}. ` +
				'Dependencies come back with `devp restore` or a normal install.\n\nClick for actions.',
		);
		statusItem.show();
		return;
	}

	if (entry.state === 'active') {
		currentState = 'active';
		statusItem.text = `$(devprune-logo) ${formatBytes(occupied)} in use`;
		statusItem.tooltip = new vscode.MarkdownString(
			`**dev-prune** — this repository is active, so nothing will be pruned.\n\n` +
				`Dependency and build folders currently occupy ${formatBytes(occupied)}.\n\n` +
				`Package managers in use: ${managers}.` +
				sharedBytesNote(entry) +
				'\n\nClick for actions.',
		);
		statusItem.show();
		return;
	}

	// Ignored, path-missing, config-error, no-bloat: state the fact plainly.
	currentState = 'other';
	statusItem.text = `$(devprune-logo) devp: ${entry.state.replace(/_/g, ' ')}`;
	statusItem.tooltip = new vscode.MarkdownString(
		`**dev-prune** — this repository is in state \`${entry.state}\`.\n\n` +
			`Package managers in use: ${managers}.\n\nClick for actions.`,
	);
	statusItem.show();
}

function refreshStatus(context, onStillMissing) {
	if (!vscode.workspace.isTrusted) {
		return;
	}
	const root = workspaceRoot();
	if (!root) {
		statusItem.hide();
		return;
	}
	execFile(binPath, ['status', '--json'], EXEC_OPTS, (error, stdout) => {
		if (error && error.code === 'ENOENT') {
			const found = probeKnownLocations();
			if (found && found !== binPath) {
				binPath = found;
				refreshStatus(context, onStillMissing);
				return;
			}
			binPath = BIN;
			showCliMissing(context);
			if (onStillMissing) onStillMissing();
			return;
		}
		if (error && error.killed) {
			// The call hit the timeout and was killed mid-answer. That is a
			// slow machine, not a missing CLI: "not found" would be wrong and
			// hiding would be silent, so say what happened and stay clickable
			// (the QuickPick's Refresh retries).
			currentState = 'status-timeout';
			currentEntry = undefined;
			statusItem.text = '$(devprune-logo) devp: not responding';
			statusItem.tooltip =
				'devp status --json did not answer within 60 seconds. Click for actions — Refresh tries again.';
			statusItem.backgroundColor = undefined;
			statusItem.show();
			return;
		}
		let doc;
		try {
			doc = JSON.parse(stdout);
		} catch {
			statusItem.hide();
			return;
		}
		renderState(doc, root);
		offerSkillOnce(context);
	});
}

// The skill teaches AI agents the tool. Offer it once when an agent skills
// directory exists on this machine without dev-prune in it — and never nag:
// one session popup, a permanent opt-out, and silence when no agent is
// installed at all.
function offerSkillOnce(context) {
	// A disabled-notifications setting must not consume the once-ever offer.
	if (!notificationsEnabled() || context.globalState.get(HIDE_SKILL_KEY)) {
		return;
	}
	const skillsDir = path.join(os.homedir(), '.claude', 'skills');
	const installed = path.join(skillsDir, 'dev-prune', 'SKILL.md');
	let offer = false;
	try {
		offer = fs.existsSync(skillsDir) && !fs.existsSync(installed);
	} catch {
		return;
	}
	if (!offer) {
		return;
	}
	context.globalState.update(HIDE_SKILL_KEY, true);
	vscode.window
		.showInformationMessage(
			'You have an AI agent skills directory, but the dev-prune skill is not installed. `devp skill` teaches your agent every command and safety rule.',
			'Install skill',
			'Not now',
		)
		.then((choice) => {
			if (choice === 'Install skill') {
				vscode.commands.executeCommand('devprune.installSkill');
			}
		});
}

function requireTrust() {
	if (vscode.workspace.isTrusted) {
		return true;
	}
	vscode.window.showWarningMessage(
		'dev-prune commands run the devp CLI, which is disabled until you trust this workspace.',
	);
	return false;
}

// Terminal commands run the same binary the status bar probed. When the probe
// resolved an absolute path (PATH lookup failed but a managed install
// directory has the CLI), a bare `devp` in the terminal would fail even though
// the status bar works — so spell out the path, quoted for spaces. PowerShell,
// the Windows default shell, treats a bare quoted string as an expression, not
// a program; the call operator makes it run there.
function cliForTerminal() {
	if (binPath === BIN) return 'devp';
	const quoted = `"${binPath}"`;
	return process.platform === 'win32' ? `& ${quoted}` : quoted;
}

function runInTerminal(commandLine) {
	const terminal = vscode.window.createTerminal({ name: 'dev-prune', cwd: workspaceRoot() });
	terminal.show();
	terminal.sendText(commandLine, true);
}

function runDevpInTerminal(args) {
	runInTerminal(`${cliForTerminal()} ${args}`);
}

// The QuickPick behind the status bar item: the state's own action first, the
// always-useful ones after.
function statusQuickPick(context) {
	const items = [];
	if (currentState === 'no-git') {
		items.push({ label: '$(source-control) Initialize a Git repository here (git init)', action: 'gitInit' });
	}
	if (currentState === 'unlinked') {
		items.push({ label: '$(plug) Register this repository (devp link .)', action: 'link' });
	}
	if (currentState === 'candidate') {
		items.push({ label: '$(trash) Prune this repository in a terminal (devp run .)', action: 'pruneHere' });
	}
	if (currentState === 'cleaned') {
		items.push({ label: '$(discard) Restore what was pruned (devp restore)', action: 'restore' });
	}
	items.push(
		{ label: '$(refresh) Refresh', action: 'refresh' },
		{ label: '$(terminal) Dry run in terminal (devp run --dry-run)', action: 'dryRun' },
		{ label: '$(list-tree) Open the dashboard (devp status)', action: 'dashboard' },
		{ label: '$(new-file) Create a .devprune.json for this repository', action: 'createConfig' },
		{ label: '$(circle-slash) Ignore this repository (never prune it)', action: 'ignore' },
		{ label: '$(hubot) Install the AI agent skill (devp skill)', action: 'skill' },
		{ label: '$(book) Open the CLI reference', action: 'docs' },
	);
	vscode.window.showQuickPick(items, { placeHolder: 'dev-prune' }).then((pick) => {
		if (!pick) return;
		if (pick.action === 'gitInit') vscode.commands.executeCommand('devprune.gitInit');
		else if (pick.action === 'link') vscode.commands.executeCommand('devprune.linkRepo');
		else if (pick.action === 'pruneHere') runDevpInTerminal('run .');
		else if (pick.action === 'restore') runDevpInTerminal('restore');
		else if (pick.action === 'refresh') refreshStatus(context);
		else if (pick.action === 'dryRun') vscode.commands.executeCommand('devprune.dryRun');
		else if (pick.action === 'dashboard') runDevpInTerminal('status');
		else if (pick.action === 'createConfig') vscode.commands.executeCommand('devprune.createConfig');
		else if (pick.action === 'ignore') vscode.commands.executeCommand('devprune.ignoreRepo');
		else if (pick.action === 'skill') vscode.commands.executeCommand('devprune.installSkill');
		else if (pick.action === 'docs') vscode.commands.executeCommand('devprune.openDocs');
	});
}

function registerCommands(context) {
	context.subscriptions.push(
		vscode.commands.registerCommand('devprune.showStatus', () => {
			if (!requireTrust()) return;
			if (currentState === 'cli-missing') {
				// The verdict may be stale: it was computed at startup, and the
				// CLI may have been installed (or restored from an antivirus
				// quarantine) since. Re-probe, and only send the user to the
				// install page when devp is still really missing.
				statusItem.text = '$(devprune-logo) devp: checking…';
				refreshStatus(context, () => {
					vscode.env.openExternal(vscode.Uri.parse(INSTALL_URL));
				});
				return;
			}
			statusQuickPick(context);
		}),
		vscode.commands.registerCommand('devprune.refresh', () => {
			if (!requireTrust()) return;
			refreshStatus(context);
		}),
		vscode.commands.registerCommand('devprune.gitInit', () => {
			if (!requireTrust()) return;
			// A visible terminal, not a silent spawn: initializing version
			// control is the user's act, and they should see it happen.
			runInTerminal('git init');
		}),
		vscode.commands.registerCommand('devprune.linkRepo', () => {
			if (!requireTrust()) return;
			runDevpInTerminal('link .');
		}),
		vscode.commands.registerCommand('devprune.dryRun', () => {
			if (!requireTrust()) return;
			// A dry run deletes nothing; it still goes to a visible terminal so
			// the command the user would run next is the one they just watched.
			runDevpInTerminal('run --dry-run');
		}),
		vscode.commands.registerCommand('devprune.createConfig', () => {
			const root = workspaceRoot();
			if (!root) return;
			const file = path.join(root, '.devprune.json');
			// The same skeleton the CLI's docs show: the $schema line wires up
			// validation and hover docs the moment the file opens. `wx` is the whole
			// existence check — it fails if the file is already there, which is the
			// same outcome an `existsSync` would have produced and cannot be raced.
			const skeleton = `{\n\t"$schema": "${SCHEMA_URL}",\n\t"override_idle_days": 15\n}\n`;
			try {
				fs.writeFileSync(file, skeleton, { flag: 'wx' });
			} catch {
				// Already exists, or another writer won the race — open what is there.
			}
			vscode.workspace.openTextDocument(file).then((doc) => vscode.window.showTextDocument(doc));
		}),
		vscode.commands.registerCommand('devprune.ignoreRepo', () => {
			const root = workspaceRoot();
			if (!root) return;
			const file = path.join(root, '.devprune.json');
			let config = {};
			try {
				config = JSON.parse(fs.readFileSync(file, 'utf8'));
			} catch {
				// No config yet, or unparseable — start from the skeleton either
				// way; a broken file is shown to the user in the editor below.
				config = { $schema: SCHEMA_URL };
			}
			config.ignore = true;
			try {
				fs.writeFileSync(file, `${JSON.stringify(config, null, '\t')}\n`);
			} catch (e) {
				vscode.window.showWarningMessage(`Could not write ${file}: ${e.message}`);
				return;
			}
			vscode.window.showInformationMessage(
				'This repository is now ignored — dev-prune will never prune it. Delete the "ignore" line in .devprune.json to undo.',
			);
			refreshStatus(context);
		}),
		vscode.commands.registerCommand('devprune.installSkill', () => {
			if (!requireTrust()) return;
			execFile(binPath, ['skill'], EXEC_OPTS, (error) => {
				if (error) {
					vscode.window.showWarningMessage(
						error.code === 'ENOENT'
							? 'devp is not on your PATH — install the CLI first.'
							: `devp skill failed: ${error.message}`,
					);
					return;
				}
				vscode.window.showInformationMessage(
					'dev-prune agent skill installed. Agents with a skills directory pick it up on their next session.',
				);
			});
		}),
		vscode.commands.registerCommand('devprune.openDocs', () => {
			vscode.env.openExternal(vscode.Uri.parse(DOCS_URL));
		}),
	);
}

function activate(context) {
	statusItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 90);
	statusItem.name = 'dev-prune';
	statusItem.command = 'devprune.showStatus';
	context.subscriptions.push(statusItem);

	registerCommands(context);

	// A cli-missing verdict is otherwise computed once per window. The install
	// happens in a terminal outside VS Code and the user comes back by
	// focusing the window, so recheck then — and only then: with a healthy
	// status bar a focus change spawns nothing.
	context.subscriptions.push(
		vscode.window.onDidChangeWindowState((state) => {
			if (state.focused && currentState === 'cli-missing' && vscode.workspace.isTrusted) {
				refreshStatus(context);
			}
		}),
	);

	if (vscode.workspace.isTrusted) {
		refreshStatus(context);
	} else {
		// Spawning nothing in untrusted workspaces; the schema mapping still works.
		context.subscriptions.push(
			vscode.workspace.onDidGrantWorkspaceTrust(() => refreshStatus(context)),
		);
	}
}

function deactivate() {}

module.exports = { activate, deactivate };
