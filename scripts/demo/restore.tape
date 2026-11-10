# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0

# The way back, on its own: a prune pass has deleted a project's node_modules, `devp
# history --pass 1` lists what it removed, `devp restore --last-run` puts it back, and the
# directory is populated again. Everything on screen is real output.
#
# Render from this directory, against the sandbox that prepare-sandbox.ps1 builds (run it
# with no -TargetFrom: this clip is npm only, because devp does not restore a Rust target/):
#   vhs restore.tape
#
# Two waits are cut out with Hide/Show, because VHS has no fast-forward: the prune pass that
# makes the deletion (done before the camera shows anything) and the `npm ci` inside the
# restore. The restore command is typed and started on camera, and the finished output is
# what comes back. Nothing between them is edited, only skipped.
#
# Every Wait matches the empty prompt, for the reasons given in demo.tape.

Source common.tape

Output out/restore.gif

# The sandbox config holds one repository. Without this line the tape would run against the
# real registry, so it is the first thing in the file and the last thing to be removed.
Hide
Type "$env:DEV_PRUNE_CONFIG_DIR='V:\devp-demo\config'; $env:DEV_PRUNE_NO_AUTO_SETUP='1'; [Console]::OutputEncoding=[Text.Encoding]::UTF8; Set-Location V:\devp-demo\dev-prune"
Enter
Type "function prompt { Write-Host -NoNewline 'dev-prune ' -ForegroundColor Cyan; Write-Host -NoNewline ([string][char]0x276F) -ForegroundColor Green; ' ' }"
Enter
Type "Set-PSReadLineOption -Colors @{ Command='Green'; Parameter='Cyan'; String='Yellow'; Comment='DarkGray'; Default='White'; Operator='White'; Number='Yellow' }; Clear-Host"
Enter
Sleep 1s

# The pass that deletes node_modules, run before the camera shows anything.
Type "devp run -y"
Enter
Sleep 5s
Wait@10m /^dev-prune ❯$/
Ctrl+L
Sleep 1s
Show

Type "# That pass deleted node_modules. Here is what it removed."
Enter
Sleep 400ms
Type "devp history --pass 1"
Enter
Sleep 7s

Ctrl+L
Type "# And it really is gone from disk."
Enter
Sleep 400ms
Type "Test-Path site\node_modules"
Enter
Sleep 3s

Type "# Put back exactly what that pass deleted."
Enter
Sleep 400ms
Type "devp restore --last-run -y"
Enter
Sleep 3s
Hide
Wait@10m /^dev-prune ❯$/
Show
Sleep 3s

Type "Test-Path site\node_modules; (Get-ChildItem site\node_modules).Count"
Enter
Sleep 8s
