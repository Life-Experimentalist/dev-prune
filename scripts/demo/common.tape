# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0

# The look shared by every demo tape: the landing page's slate ground, its green
# for "proven rebuildable" and its gold for "space you get back".

Set Shell "powershell"
Set FontFamily "Cascadia Code"
Set FontSize 17
Set LetterSpacing 0
Set LineHeight 1.2
Set Width 1100
Set Height 700
Set Padding 28
Set Margin 36
Set MarginFill "#0b0e17"
Set BorderRadius 14
Set WindowBar Colorful
Set WindowBarSize 44
Set TypingSpeed 50ms
Set CursorBlink false

# VHS on Windows records frames as fast as Chromium can screenshot them. At its default
# 50 fps the capture falls behind and the recording plays back 3-4x too fast; at 15 it
# keeps up and the timing in the file matches the timing of the tape.
Set Framerate 15

Set Theme { "name": "devprune", "black": "#171b2a", "red": "#ff5c7a", "green": "#10b981", "yellow": "#ffc857", "blue": "#4c9aff", "magenta": "#9f7aea", "cyan": "#2bd9d9", "white": "#e8ecf7", "brightBlack": "#6b7592", "brightRed": "#ff8099", "brightGreen": "#34d399", "brightYellow": "#ffd98a", "brightBlue": "#7db4ff", "brightMagenta": "#b79cf0", "brightCyan": "#6ee7e7", "brightWhite": "#ffffff", "background": "#10131e", "foreground": "#e8ecf7", "selection": "#262c40", "cursor": "#10b981" }
