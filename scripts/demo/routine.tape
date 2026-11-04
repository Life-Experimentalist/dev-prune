# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0

# The background routine: the log and the lifetime totals of a machine that has been running
# dev-prune on its scheduler for weeks. Read-only and recorded against the real registry, so
# it runs `history` and `stats` and nothing that can delete.
#
# `devp stats` is cut to its first lines with Select-Object because the rest of the report
# names the repositories on the machine it was recorded on. Run from this directory:
#   vhs routine.tape

Source common.tape

Output out/routine.gif
Output out/routine.mp4
Output out/routine.webm

Hide
Type "$env:DEV_PRUNE_NO_AUTO_SETUP='1'; [Console]::OutputEncoding=[Text.Encoding]::UTF8; Set-Location C:\"
Enter
Type "function prompt { Write-Host -NoNewline 'dev-prune ' -ForegroundColor Cyan; Write-Host -NoNewline ([string][char]0x276F) -ForegroundColor Green; ' ' }"
Enter
Type "Set-PSReadLineOption -Colors @{ Command='Green'; Parameter='Cyan'; String='Yellow'; Comment='DarkGray'; Default='White'; Operator='White'; Number='Yellow' }; Clear-Host"
Enter
Sleep 1s
Show

Type "# Nobody typed the last fourteen of these. The scheduler did."
Enter
Sleep 500ms
Type "devp history --limit 14"
Enter
Sleep 7s

Ctrl+L
Type "devp stats | Select-Object -First 22"
Enter
Sleep 8s
