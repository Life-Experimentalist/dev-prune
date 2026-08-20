// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// The schema mapping in package.json needs no code. This file exists for one
// thing: a workspace that contains .devprune.json but has no devp on PATH
// looks like dev-prune is working when nothing is acting on the file at all,
// so say so once instead of staying silent.

const { execFile } = require('node:child_process');
const os = require('node:os');
const vscode = require('vscode');

const HIDE_KEY = 'devprune.hideCliMissingNotice';
const INSTALL_URL = 'https://devprune.vkrishna04.me';

function checkCli(context) {
	const bin = process.platform === 'win32' ? 'devp.exe' : 'devp';
	// cwd is pinned outside the workspace so the lookup can never resolve
	// through workspace content on platforms that search cwd first.
	execFile(bin, ['-V'], { cwd: os.homedir(), timeout: 5000 }, (error) => {
		if (!error || error.code !== 'ENOENT') {
			return;
		}
		vscode.window
			.showInformationMessage(
				'This workspace has a .devprune.json, but the dev-prune CLI (devp) is not on your PATH — nothing is acting on that file. Install the CLI to reclaim disk space from idle repositories.',
				'Install instructions',
				"Don't show again",
			)
			.then((choice) => {
				if (choice === 'Install instructions') {
					vscode.env.openExternal(vscode.Uri.parse(INSTALL_URL));
				} else if (choice === "Don't show again") {
					context.globalState.update(HIDE_KEY, true);
				}
			});
	});
}

function activate(context) {
	if (context.globalState.get(HIDE_KEY)) {
		return;
	}
	if (vscode.workspace.isTrusted) {
		checkCli(context);
	} else {
		// Spawning nothing in untrusted workspaces; the schema mapping still works.
		context.subscriptions.push(
			vscode.workspace.onDidGrantWorkspaceTrust(() => checkCli(context)),
		);
	}
}

function deactivate() {}

module.exports = { activate, deactivate };
