# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0

# The hero demo: a Rust project's 13 GiB target/ and an npm project's node_modules go away
# after their lockfiles are verified, and come back (the npm half is reinstalled, and
# cargo rebuilds target/ the next time you build). Everything on screen is real output.
#
# Render from this directory, against the sandbox that prepare-sandbox.ps1 builds:
#   vhs demo.tape
#
# Two waits are cut out with Hide/Show, because VHS has no fast-forward and they are long:
# the delete (about three minutes, 21,000 files under Defender) and the npm restore (about
# two). The command is typed and started on camera, and the finished output is what comes
# back. Nothing between them is edited, only skipped.
#
# Every Wait matches the empty prompt rather than the command's last line of output. `Wait`
# only looks at the screen's last line, which is the prompt once a command has finished, and
# `Wait+Screen` read a stale screen in testing and timed out on output that was plainly there.

Source common.tape

Output out/demo.gif
Output out/demo.mp4
Output out/demo.webm

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
Show

Type "# A Rust project and an npm project, both sitting on disk."
Enter
Sleep 600ms

Type "devp run --dry-run"
Enter
Wait@10m /^dev-prune ❯$/
Sleep 4s

Ctrl+L
Type "devp run -y"
Enter
Sleep 5s
Hide
Wait@10m /^dev-prune ❯$/
Show
Sleep 3500ms

Ctrl+L
Type "# Changed your mind? npm reinstalls, cargo rebuilds on demand."
Enter
Sleep 400ms
Type "devp restore --last-run -y"
Enter
Sleep 3s
Hide
Wait@10m /^dev-prune ❯$/
Show
Sleep 4s
