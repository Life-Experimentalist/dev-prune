# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0

# The refusal: a project whose package.json has gained a dependency its lockfile has never
# heard of. `node_modules` is 60 MiB and idle, and `devp run -y` still deletes nothing,
# because the lockfile could not put it back. This is the clip for "will it come back?".
#
# prepare-sandbox.ps1 builds the fixture (a second sandbox repository with its own config
# directory, so the take can be repeated: nothing is ever deleted). npm's cache is pointed
# into the sandbox because npm prints its log path, which would otherwise show a home
# directory. Run from this directory:
#   vhs refusal.tape

Source common.tape

Output out/refusal.gif
Output out/refusal.mp4
Output out/refusal.webm

Hide
Type "$env:DEV_PRUNE_CONFIG_DIR='V:\devp-demo\config-refuse'; $env:DEV_PRUNE_NO_AUTO_SETUP='1'; $env:npm_config_cache='V:\devp-demo\npm-cache'; [Console]::OutputEncoding=[Text.Encoding]::UTF8; Set-Location V:\devp-demo\side-project"
Enter
Type "function prompt { Write-Host -NoNewline 'side-project ' -ForegroundColor Cyan; Write-Host -NoNewline ([string][char]0x276F) -ForegroundColor Green; ' ' }"
Enter
Type "Set-PSReadLineOption -Colors @{ Command='Green'; Parameter='Cyan'; String='Yellow'; Comment='DarkGray'; Default='White'; Operator='White'; Number='Yellow' }; Clear-Host"
Enter
Sleep 1s
Show

Type "# is-odd is in package.json. The lockfile has never heard of it."
Enter
Sleep 500ms
Type "devp run -y"
Enter
Sleep 3s
Hide
Wait@5m /^side-project ❯$/
Show
Sleep 5s

Ctrl+L
Type "# Nothing was deleted."
Enter
Type "Test-Path node_modules"
Enter
Sleep 4s
