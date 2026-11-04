# Demo recordings

The clips in the root [`README.md`](../../README.md) and on the landing page are recorded
from the real binary with [VHS](https://github.com/charmbracelet/vhs), from the tapes in
this directory. Nothing is mocked: every line on screen is output `devp` printed.

| Tape | What it shows | Runs against |
|---|---|---|
| [`demo.tape`](demo.tape) | `devp run` deleting a 13 GiB Rust `target/` and an npm `node_modules`, then `devp restore --last-run` | a sandbox clone, never your registry |
| [`refusal.tape`](refusal.tape) | `devp run -y` refusing a project whose lockfile is out of sync: frees 0 B and `node_modules` stays | a second sandbox repository with its own registry; nothing is deleted, so retakes need no re-prep |
| [`routine.tape`](routine.tape) | `devp history` and `devp stats` from a machine that has run the scheduler for weeks | the recording machine's real registry, read-only |
| [`common.tape`](common.tape) | the window, font and colours both tapes share | n/a |

## What was changed to make them watchable

Two things, and the clips say so beside them.

**Two waits are cut.** Deleting about 21,000 files takes roughly three minutes on Windows
with Defender scanning, and the npm half of `devp restore` runs `npm ci`, which takes
about two. VHS cannot fast-forward, so the tape hides the terminal while it waits for the
output to appear (`Hide`, `Wait`, `Show`). The command is typed and started on camera and
the finished output is what comes back. The numbers on screen are the numbers the run
printed.

**Idle days are zero in the sandbox.** A project built five minutes ago would otherwise be
skipped as active. [`prepare-sandbox.ps1`](prepare-sandbox.ps1) sets `idle_days` and
`build_idle_days` to `0` in the sandbox's own config so the clone is eligible. Every
other check is the shipped one: the lockfile is verified, the rebuild command is
resolved, symlinks and the `.git` boundary are refused. The sandbox holds one repository.

## Why the routine clip is cut short

`devp stats` also prints which repositories on the machine freed the most space, and
`devp history --pass N` lists the directories each pass deleted. Both name projects on
the recording machine, so the tape uses `devp history --limit` and the head of `devp
stats` only. If you re-record it on a machine with private project names, keep it that
way.

## Re-recording

You need `vhs`, `ttyd` and `ffmpeg` on `PATH`, `devp` installed, and the Cascadia Code
font. VHS records one frame per Chromium screenshot, so keep the machine otherwise idle:
a disk scan running during a take stretches the dry-run wait and can fail it.

```powershell
# Build the sandbox. Pass a finished build of this repository to avoid compiling one.
pwsh scripts\demo\prepare-sandbox.ps1 -TargetFrom V:\Code\ProjectCode\dev-prune\target

cd scripts\demo
vhs demo.tape      # about 15 minutes, most of it the two cut waits
vhs routine.tape   # about a minute
vhs refusal.tape   # about a minute
```

A take deletes the sandbox's `target\` and `node_modules`, so run `prepare-sandbox.ps1`
again before the next one. Output lands in `out/` (ignored by Git).

`Set Framerate 15` in [`common.tape`](common.tape) is load-bearing. At VHS's default 50
fps the capture cannot keep up on Windows and the file plays back three to four times
too fast.

The version in the banner is the version of the `devp` you recorded with. Re-record after
a release if the banner matters to you.

## Publishing the result

The GIF is the README's copy, because GitHub does not play video from a relative path in
a README. The WebM and MP4 are the landing page's, where a `<video>` can be paused and
costs less to decode. Both clips are mostly flat dark pixels, so the GIFs VHS writes are
already small (about 550 KB and 420 KB) and are published as they come.

| From `out/` | To | Used by |
|---|---|---|
| `demo.gif`, `routine.gif`, `refusal.gif` | [`assets/`](../../assets/) | the root README |
| `demo.webm`, `demo.mp4`, and a `demo-poster.jpg` frame | `site/public/assets/demo/` | the landing page |

The poster is the dry-run summary frame, taken from the MP4:

```powershell
ffmpeg -y -ss 9 -i out\demo.mp4 -frames:v 1 -q:v 3 ..\..\site\public\assets\demo\demo-poster.jpg
```

The PNGs in [`assets/screenshots/`](../../assets/screenshots/) are single frames cut the same way, with no filter. After a re-render the timestamps move, so look at a one-frame-per-second contact sheet first (`-vf "fps=1,tile=4x3"`) rather than reusing old offsets.

If a re-recording ever pushes a GIF past a few megabytes, an ffmpeg palette pass
(`fps=12`, `palettegen=max_colors=96:stats_mode=diff`, `paletteuse=dither=none`) brings it
back down without banding the dark theme.
