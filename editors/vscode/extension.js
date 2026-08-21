// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// The schema mapping in package.json needs no code. This file adds the parts
// that do: a status bar item showing what devp could reclaim in this
// workspace, a small set of read-only palette commands, and two notices — one
// when .devprune.json exists but devp is not on PATH (nothing is acting on
// the file), one offering the AI agent skill when an agent skills directory
// exists without it. Anything that deletes stays in the terminal, spelled out
// where the user can read it before running it.

const { execFile } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const vscode = require('vscode');

const HIDE_KEY = 'devprune.hideCliMissingNotice';
const HIDE_SKILL_KEY = 'devprune.hideSkillOfferNotice';
const INSTALL_URL = 'https://devprune.vkrishna04.me';
const DOCS_URL = 'https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/CLI_REFERENCE.md';

const BIN = process.platform === 'win32' ? 'devp.exe' : 'devp';
// cwd is pinned outside the workspace so the lookup can never resolve
// through workspace content on platforms that search cwd first.
const EXEC_OPTS = { cwd: os.homedir(), timeout: 15000 };

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
let cliMissing = false;
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

function showCliMissing(context) {
	cliMissing = true;
	// The activation globs also match ignore.devprune.json — the opt-out
	// marker. A workspace that only opted out has nothing for devp to act on,
	// so it gets neither the warning nor the install nag: verify a real config
	// file exists before claiming one does.
	vscode.workspace.findFiles('**/.devprune.json', undefined, 1).then((found) => {
		if (!found || found.length === 0) {
			statusItem.hide();
			return;
		}
		statusItem.text = '$(warning) devp not found';
		statusItem.tooltip =
			'This workspace has a .devprune.json, but the dev-prune CLI (devp) is not on your PATH — nothing is acting on that file.';
		statusItem.backgroundColor = new vscode.ThemeColor('statusBarItem.warningBackground');
		statusItem.show();
		if (noticeShownThisSession || context.globalState.get(HIDE_KEY) || !notificationsEnabled()) {
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

function refreshStatus(context, onStillMissing) {
	if (!vscode.workspace.isTrusted) {
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
		cliMissing = false;
		statusItem.backgroundColor = undefined;
		let doc;
		try {
			doc = JSON.parse(stdout);
		} catch {
			statusItem.hide();
			return;
		}
		const totals = doc.totals || {};
		const bytes = totals.reclaimable_bytes || 0;
		const candidates = totals.candidates || 0;
		statusItem.text = `$(trash) ${formatBytes(bytes)}`;
		statusItem.tooltip = new vscode.MarkdownString(
			`**dev-prune** — ${formatBytes(bytes)} reclaimable across ` +
				`${totals.repositories || 0} registered repositories, ` +
				`${candidates} ready to prune now.\n\nClick for actions.`,
		);
		statusItem.show();
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

function runInTerminal(args) {
	const terminal = vscode.window.createTerminal({ name: 'dev-prune', cwd: workspaceRoot() });
	terminal.show();
	terminal.sendText(`${cliForTerminal()} ${args}`, true);
}

function registerCommands(context) {
	context.subscriptions.push(
		vscode.commands.registerCommand('devprune.showStatus', () => {
			if (!requireTrust()) return;
			if (cliMissing) {
				vscode.env.openExternal(vscode.Uri.parse(INSTALL_URL));
				return;
			}
			vscode.window
				.showQuickPick(
					[
						{ label: '$(refresh) Refresh reclaimable size', action: 'refresh' },
						{ label: '$(terminal) Dry run in terminal (devp run --dry-run)', action: 'dryRun' },
						{ label: '$(list-tree) Open the dashboard (devp status)', action: 'dashboard' },
						{ label: '$(hubot) Install the AI agent skill (devp skill)', action: 'skill' },
						{ label: '$(book) Open the CLI reference', action: 'docs' },
					],
					{ placeHolder: 'dev-prune' },
				)
				.then((pick) => {
					if (!pick) return;
					if (pick.action === 'refresh') refreshStatus(context);
					else if (pick.action === 'dryRun') vscode.commands.executeCommand('devprune.dryRun');
					else if (pick.action === 'dashboard') runInTerminal('status');
					else if (pick.action === 'skill') vscode.commands.executeCommand('devprune.installSkill');
					else if (pick.action === 'docs') vscode.commands.executeCommand('devprune.openDocs');
				});
		}),
		vscode.commands.registerCommand('devprune.dryRun', () => {
			if (!requireTrust()) return;
			// A dry run deletes nothing; it still goes to a visible terminal so
			// the command the user would run next is the one they just watched.
			runInTerminal('run --dry-run');
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
