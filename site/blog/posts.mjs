// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// The guide pages.
//
// These exist to be found. Somebody types "is it safe to delete node_modules" into a
// search engine at the moment their disk fills up, and the honest answer to that question
// is also, eventually, an argument for this tool — so the answer comes first and the tool
// comes last, on every page. A guide that withholds the answer to sell the product is a
// guide nobody links to, and links are the entire mechanism.
//
// Each entry is plain HTML rather than Markdown: there is no build-time Markdown
// dependency in this repository and adding one to render seven files would be a strange
// trade. `build-blog.mjs` wraps these in the site shell.
//
// House rules for anything added here:
//   - Every claim about dev-prune must be true of the shipped binary. No roadmap
//     written in the present tense.
//   - No invented numbers. The one measured figure on the site is the author's own
//     machine and it is labelled as such.
//   - No backticks or ${} inside the template literals below.

export const SITE = 'https://devprune.vkrishna04.me';
export const UPDATED = '2026-08-22';

export const POSTS = [
  {
    slug: 'safe-to-delete-node-modules',
    title: 'Is it safe to delete node_modules?',
    description:
      'Yes — as long as the lockfile is intact and you can reinstall. Here is what actually breaks, what does not, and how to check before you delete.',
    keywords:
      'delete node_modules, is it safe to delete node_modules, node_modules disk space',
    body: `
<p><strong>Short answer: yes.</strong> <code>node_modules</code> is a build product. Nothing
in it is authored by you, and every file in it is described by your lockfile
(<code>package-lock.json</code>, <code>pnpm-lock.yaml</code>, <code>yarn.lock</code> or
<code>bun.lock</code>). Delete it, run your install command, and you get the same tree
back, byte for byte, because that is precisely what a lockfile is for.</p>

<p>The long answer is the interesting one, because "the lockfile describes it" is a claim
that is sometimes false, and the cases where it is false are exactly the cases where
deleting hurts.</p>

<h2>What you are actually deleting</h2>

<p>A <code>node_modules</code> directory holds three kinds of thing:</p>

<ul>
  <li><strong>Downloaded packages.</strong> Fetched from a registry, addressed by a hash
    the lockfile records. These come back exactly.</li>
  <li><strong>Build output from install scripts.</strong> Native modules compiled by
    <code>node-gyp</code>, binaries downloaded by a postinstall step, prebuilt artefacts
    unpacked into place. These come back <em>if</em> the machine can still build or fetch
    them — which needs a compiler, or network access to whatever the postinstall step
    reaches for.</li>
  <li><strong>Whatever you edited by hand.</strong> Which is nothing, unless you have been
    debugging a dependency by editing it in place. If you have, that work is not in any
    lockfile and it is gone the moment you delete.</li>
</ul>

<p>So the honest rule is not "node_modules is always safe to delete". It is: <em>node_modules
is safe to delete when the lockfile can rebuild it and you have not modified it.</em></p>

<h2>Check before you delete</h2>

<p>Every package manager has a command that answers the question without installing
anything. Run the one that matches your lockfile:</p>

<pre><code>npm ci --dry-run --ignore-scripts
pnpm install --lockfile-only
yarn install --immutable --immutable-cache
bun install --frozen-lockfile --dry-run</code></pre>

<p>If it exits zero, the lockfile is coherent with <code>package.json</code> and the
registry can serve every version it names. If it fails, do not delete — a failure here
means reinstalling would <em>not</em> give you what you have. The two failures worth
knowing:</p>

<ul>
  <li><strong>A package in <code>node_modules</code> is missing from the lockfile.</strong>
    Usually somebody ran <code>npm install &lt;pkg&gt;</code> and committed
    <code>package.json</code> without the lockfile. Deleting loses that package until
    somebody notices.</li>
  <li><strong>A version in the lockfile no longer exists.</strong> Unpublished, or a
    private registry that has rotated. Your installed copy is now the only copy you have.</li>
</ul>

<h2>What deleting costs you</h2>

<p>Time, and only time — but the amount varies more than people expect. A warm cache and a
pnpm store make reinstalling a matter of hardlinking, and it finishes in seconds. A cold
cache on a slow connection with three native modules to compile is minutes. This is why
clearing your package manager cache and deleting <code>node_modules</code> are not the same
decision and should not be made together: the cache is the thing that makes the delete
cheap.</p>

<h2>The monorepo footnote</h2>

<p>In a workspace, <code>node_modules</code> exists at the root <em>and</em> inside packages,
and the nested ones are often symlinks into the root. Deleting a symlink is harmless;
deleting through one is how people lose the root store by accident. If you are writing your
own cleanup, use a tool that refuses to follow symlinks rather than one that resolves them.</p>

<h2>Doing it across every project at once</h2>

<p>One project is a <code>rm -rf</code>. Thirty projects is a script, and a script that does
not run the verification above is a script that will eventually delete something that does
not come back.</p>

<p>That is the job <a href="/">dev-prune</a> does: it walks your Git repositories, runs the
matching dry-run for whichever lockfile it finds, and deletes only after that command exits
zero. When it does not, it says which package failed and moves on rather than deleting
anyway. <a href="/delete-node-modules-all-projects/">Deleting node_modules from every
project at once</a> covers that case in full.</p>
`,
    faq: [
      {
        q: 'Is it safe to delete node_modules?',
        a: 'Yes, provided the lockfile is intact and you have not edited anything inside it by hand. node_modules is a build product: running npm ci, pnpm install, yarn install or bun install rebuilds it from the lockfile. Verify first with npm ci --dry-run --ignore-scripts (or the equivalent for your manager) — if that fails, reinstalling will not give you back what you currently have.',
      },
      {
        q: 'Will deleting node_modules break my project?',
        a: 'Not the project — only your ability to run it until you reinstall. Your source, your package.json and your lockfile are untouched. The exception is a dependency you have edited in place while debugging, which no lockfile records and which will not come back.',
      },
      {
        q: 'Do I need to delete package-lock.json too?',
        a: 'No, and you should not. The lockfile is what makes deleting node_modules reversible. Deleting both turns a reinstall into a re-resolve, which can pick up different versions than you had.',
      },
    ],
    related: ['delete-node-modules-all-projects', 'clear-package-manager-cache'],
  },

  {
    slug: 'delete-node-modules-all-projects',
    title: 'How to delete node_modules from every project at once',
    description:
      'find, npkill, and a lockfile-verified alternative — the three ways to clear node_modules across a whole machine, and what each one risks.',
    keywords:
      'delete all node_modules, remove node_modules recursively, npkill, find node_modules delete',
    body: `
<p>If you have thirty repositories on a laptop, you have thirty <code>node_modules</code>
directories, and most of them belong to projects you have not opened in months. Here are
the three ways people clear them, in increasing order of how much they check first.</p>

<h2>1. find, or the PowerShell equivalent</h2>

<pre><code>find ~/Code -name node_modules -type d -prune -exec rm -rf {} +</code></pre>

<p>On Windows PowerShell:</p>

<pre><code>Get-ChildItem ~/Code -Recurse -Directory -Filter node_modules |
  ForEach-Object { Remove-Item $_.FullName -Recurse -Force }</code></pre>

<p>It works, it takes one line, and it verifies nothing. It will happily delete the
<code>node_modules</code> of the project you are debugging right now, and the one whose
lockfile is out of sync and will not reinstall. <code>-prune</code> in the
<code>find</code> version matters: without it, <code>find</code> descends into directories
it is about to delete and wastes minutes walking a tree it will discard.</p>

<h2>2. npkill</h2>

<pre><code>npx npkill</code></pre>

<p><a href="https://github.com/voidcosmos/npkill" rel="noopener" target="_blank">npkill</a> is
the well-known one, and it is good at what it does: it scans, sorts by size, shows you the
last-modified date, and lets you delete interactively with the arrow keys. If you want to
look at a list and make thirty individual decisions, this is the tool.</p>

<p>What it does not do is check whether any given directory can be rebuilt. The last-modified
date is a proxy for "probably safe" and it is a decent one, but it is a guess, and it is
your guess to make on each row.</p>

<h2>3. Verify, then delete</h2>

<p>The third option is to make the check the tool's job rather than yours. That is what
<a href="/">dev-prune</a> is: it walks the Git repositories you have registered, and for each
one it runs the dry-run that matches the lockfile it found —
<code>npm ci --dry-run --ignore-scripts</code>, <code>pnpm install --lockfile-only</code>,
<code>yarn install --immutable</code>, <code>bun install --frozen-lockfile --dry-run</code> —
<em>before</em> deleting anything. If that command exits non-zero, the directory stays, and
you are told which package was missing from the lockfile.</p>

<pre><code>curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
devp init ~/Code
devp run --dry-run</code></pre>

<p>The other three checks it makes are worth knowing, because they are the ones a
hand-rolled script never has:</p>

<ul>
  <li><strong>It will not cross a <code>.git</code> boundary.</strong> A nested repository
    inside a repository is a separate decision, not part of the outer one.</li>
  <li><strong>It refuses symlinks outright.</strong> It never follows one and never deletes
    through one, which is the failure mode that costs people a pnpm store.</li>
  <li><strong>It only touches repositories that have been idle.</strong> Idle means no
    commit and no working-tree change for a configurable number of days. The project you
    are working in today is not a candidate, whatever its size.</li>
</ul>

<p>And it is not only Node: the same pass handles <code>.venv</code> for uv, Poetry, PDM,
Pipenv and plain venv, <code>vendor/</code> for Go, Composer and Bundler,
<code>deps/</code> for Mix, <code>Pods/</code> for CocoaPods — twenty-three package
managers in total, fifteen of them on by default.</p>

<h2>Which to use</h2>

<p>Clearing one project you are looking at: <code>rm -rf node_modules</code>. Nothing beats
it. Going through a machine by hand once, deciding case by case: npkill. Wanting the machine
to stay clean without you thinking about it, and wanting a refusal rather than a deletion
when something is off: that is the case dev-prune was written for. There is a fuller
comparison, including <code>cargo-sweep</code>, <code>rimraf</code> and the rest, on
<a href="/vs/">dev-prune vs the alternatives</a>.</p>
`,
    faq: [
      {
        q: 'How do I delete all node_modules folders recursively?',
        a: 'On macOS or Linux: find ~/Code -name node_modules -type d -prune -exec rm -rf {} + — the -prune is what stops find from descending into directories it is about to delete. On Windows: Get-ChildItem ~/Code -Recurse -Directory -Filter node_modules | ForEach-Object { Remove-Item $_.FullName -Recurse -Force }. Neither checks whether the directories can be reinstalled.',
      },
      {
        q: 'What is the difference between npkill and dev-prune?',
        a: 'npkill scans for node_modules directories and lets you delete them interactively, sorted by size and last-modified date. dev-prune runs the package manager dry-run that matches each lockfile and deletes only when it exits zero, across twenty-three package managers rather than Node alone, and skips repositories that are not idle.',
      },
      {
        q: 'Is there a way to delete node_modules automatically?',
        a: 'dev-prune registers a background pass with the OS scheduler — Task Scheduler on Windows, launchd on macOS, systemd or cron on Linux — that runs every two days and prunes only repositories that have been idle past your threshold and whose lockfiles verify.',
      },
    ],
    related: ['safe-to-delete-node-modules', 'vs'],
  },

  {
    slug: 'reclaim-disk-space-developer-machine',
    title: 'Where a developer machine’s disk actually goes',
    description:
      'Dependency trees, build directories and package manager caches, ranked by how much they hold and how cheaply they come back.',
    keywords:
      'reclaim disk space developer, mac running out of disk space developer, free up disk space programming',
    body: `
<p>When a developer machine fills up, the answer is almost never photos. It is a few
thousand directories that were downloaded or compiled, are all individually reasonable, and
have never once been deleted. Here is the ranking, and the thing that matters about each:
how much it holds versus how expensive it is to get back.</p>

<h2>1. Dependency directories</h2>

<p><code>node_modules</code>, <code>.venv</code>, <code>vendor/</code>, <code>Pods/</code>,
<code>deps/</code>. These are the largest total and the cheapest to restore, because a
lockfile describes every byte of them and restoring is a download you have probably already
cached. One <code>node_modules</code> is tens to hundreds of megabytes; thirty of them is
the single biggest number on most machines.</p>

<p>They are also the safest thing to delete, and the reason is worth stating plainly:
<em>you can prove it</em>. Every one of these managers has a command that verifies the
lockfile without installing, so "can this be rebuilt" is a question with a real answer
rather than a guess. <a href="/safe-to-delete-node-modules/">Is it safe to delete
node_modules</a> goes through those commands.</p>

<h2>2. Build directories</h2>

<p>Rust's <code>target/</code>, Gradle's <code>build/</code> and <code>.gradle/</code>,
Maven's <code>target/</code>, SwiftPM's <code>.build/</code>. Individually these are the
biggest single directories you own — a Rust workspace's <code>target/</code> passing several
gigabytes is ordinary, and <a href="/cargo-target-directory-size/">there is a reason for
that</a>.</p>

<p>But they are not the same decision as a dependency directory, because getting one back is
a <em>compile</em>, not a download. Deleting a 4&nbsp;GB <code>target/</code> reclaims
4&nbsp;GB and costs you a cold build the next time you touch that project. That is a fine
trade for something you have not opened since spring and a bad one for last week's work,
which is why dev-prune leaves all four of these off unless you turn them on
(<code>devp config</code>), and gives them their own idle window when you do.</p>

<h2>3. Package manager caches</h2>

<p><code>~/.npm</code>, the pnpm store, <code>~/.cargo/registry</code>,
<code>$GOMODCACHE</code>, <code>~/.m2/repository</code>, <code>~/.gradle/caches</code>.
Shared, machine-wide, and frequently several gigabytes each.</p>

<p>These are the ones to be careful with, and not for the reason people assume. They are
safe — everything in them re-downloads. The problem is that <em>the cache is what makes
everything else in this list cheap to delete</em>. Clear <code>node_modules</code> across
thirty repositories with a warm npm cache and the restores are near-instant; clear the cache
too and every one of those restores becomes a network round trip. Delete dependency
directories freely; treat the cache as a separate, occasional decision.
<a href="/clear-package-manager-cache/">Clearing package manager caches safely</a> has the
per-manager commands.</p>

<h2>4. Everything else</h2>

<p>Docker images, iOS simulators and Xcode DerivedData, old toolchain versions from rustup,
nvm and pyenv. Genuinely large, genuinely reclaimable, and each has its own vendor command
that does it properly — <code>docker system prune</code>, <code>xcrun simctl delete
unavailable</code>, <code>rustup toolchain uninstall</code>. Worth a pass once a year;
not worth automating.</p>

<h2>Doing this on a schedule instead of in a panic</h2>

<p>The problem with a manual cleanup is that it happens when the disk is already full,
which is the worst moment to be making decisions about what is safe to delete. The
alternative is to make it continuous and boring: a background pass that only ever touches
repositories that are idle, only deletes what a lockfile can rebuild, and refuses when it
cannot prove that.</p>

<pre><code>curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
devp init ~/Code
devp run --dry-run</code></pre>

<p><code>--dry-run</code> deletes nothing and prints the total. That is the number worth
looking at before deciding whether any of this is a problem you have.</p>
`,
    faq: [
      {
        q: 'What takes up the most disk space on a developer machine?',
        a: 'Dependency directories in aggregate — node_modules, .venv, vendor/ — because there is one per project and they are never deleted. The largest single directories are usually build output: a Rust target/ or a Gradle build/ can pass several gigabytes on its own. Package manager caches are third, and are shared machine-wide rather than per project.',
      },
      {
        q: 'Is it safe to delete build directories like target/ and build/?',
        a: 'Safe, but not cheap. Nothing in them is authored by you, so they always regenerate — but they regenerate by compiling rather than downloading, so deleting one costs you a cold build. That trade is worth making for a project you have not touched in months and rarely worth it for current work.',
      },
    ],
    related: ['clear-package-manager-cache', 'cargo-target-directory-size'],
  },

  {
    slug: 'clear-package-manager-cache',
    title: 'Clearing package manager caches, safely',
    description:
      'The right command for npm, pnpm, yarn, bun, uv, pip, cargo, go, Maven, Gradle, NuGet, vcpkg and Conan — and why you should clear them less often than you think.',
    keywords:
      'clear npm cache, pnpm store prune, go clean modcache, cargo registry cache size, clear pip cache',
    body: `
<p>Package manager caches are the least dangerous thing on your disk to delete and the most
frequently deleted for the wrong reason. Everything in one re-downloads. Nothing in one is
unique. The catch is what a cache is <em>for</em>, which is making every other cleanup
cheap.</p>

<h2>The commands</h2>

<p>Use the manager's own subcommand where one exists. It knows which parts of its cache are
reconstructible and which are indexes it would rather rebuild itself:</p>

<pre><code>npm cache clean --force
pnpm store prune
yarn cache clean
bun pm cache rm
uv cache prune
pip cache purge
go clean -modcache
go clean -cache</code></pre>

<p>Some managers have no such command, and the honest answer there is a deletion. These are
directories, and removing them is exactly what the manager would do if it had a flag for
it:</p>

<pre><code>rm -rf ~/.cargo/registry/cache
rm -rf ~/.cargo/registry/src
rm -rf ~/.m2/repository
rm -rf ~/.gradle/caches
rm -rf ~/.gradle/wrapper/dists</code></pre>

<p>On Windows PowerShell:</p>

<pre><code>Remove-Item -Recurse -Force $env:USERPROFILE\\.cargo\\registry\\cache
Remove-Item -Recurse -Force $env:USERPROFILE\\.m2\\repository
Remove-Item -Recurse -Force $env:USERPROFILE\\.gradle\\caches</code></pre>

<h2>The two cargo directories are not the same</h2>

<p><code>~/.cargo/registry/cache</code> holds the downloaded <code>.crate</code> archives.
<code>~/.cargo/registry/src</code> holds those archives unpacked. Deleting
<code>src/</code> is nearly free — cargo re-extracts from <code>cache/</code> without
touching the network. Deleting <code>cache/</code> means downloading again. If you want
space back cheaply, delete <code>src/</code> and leave <code>cache/</code> alone.</p>

<h2>pnpm's store is not really a cache</h2>

<p>The pnpm store is content-addressed and <em>hardlinked</em> into every
<code>node_modules</code> on the machine. That is what makes a pnpm install nearly
instantaneous. <code>pnpm store prune</code> removes only entries no project references any
more, which is the right command; deleting the store directory outright turns your next
install in every pnpm project into a full download.</p>

<h2>Why you should do this less often</h2>

<p>Here is the thing worth internalising: <strong>the cache is the reason deleting
node_modules is cheap.</strong> Clearing thirty <code>node_modules</code> directories with a
warm cache costs you a few seconds of hardlinking per project. Clearing the cache in the
same sitting turns every one of those restores into a network round trip, and now you have
spent an afternoon reclaiming space that would have come back anyway.</p>

<p>So: clear dependency directories often; clear caches rarely, and separately, and when
you actually need the space rather than as part of a routine.</p>

<h2>Seeing all of them at once</h2>

<p>This is why <a href="/">dev-prune</a> never touches a cache during a prune, and instead
gives caches their own command:</p>

<pre><code>devp caches</code></pre>

<p>It reports sixteen caches across thirteen managers — npm, pnpm, yarn, bun, uv, pip,
cargo, go, Maven, Gradle, NuGet, vcpkg and Conan — with the size of each and the exact
command that clears it, asking the manager itself where its cache lives rather than
guessing at a path. <code>devp caches clear &lt;manager&gt;</code> empties one when you type
it, after showing what goes and asking. A shared cache cannot be proven recoverable by any
one project's lockfile, so it is never part of an automatic pass. That is the whole
design.</p>
`,
    faq: [
      {
        q: 'Is it safe to clear the npm cache?',
        a: 'Yes. Everything in the npm cache is re-downloadable from the registry. npm cache clean --force is the supported command. The only cost is that your next install in every project has to download rather than read from disk.',
      },
      {
        q: 'How do I check how big my package manager caches are?',
        a: 'Each manager can tell you where its cache lives — npm config get cache, pnpm store path, go env GOMODCACHE, uv cache dir — and you can size that directory. devp caches does it for sixteen caches across thirteen managers in one command, including the ones with no cache-size subcommand of their own.',
      },
      {
        q: 'Should I clear caches and delete node_modules at the same time?',
        a: 'No. The cache is what makes reinstalling node_modules fast. Do the dependency directories routinely and treat the caches as a separate, occasional decision, otherwise every restore becomes a download.',
      },
    ],
    related: ['reclaim-disk-space-developer-machine', 'safe-to-delete-node-modules'],
  },

  {
    slug: 'delete-venv-safely',
    title: 'Deleting .venv safely',
    description:
      'A virtual environment is disposable by design — but only if the lockfile names everything inside it. How to tell the difference for uv, Poetry, PDM, Pipenv and plain venv.',
    keywords:
      'delete venv, is it safe to delete .venv, python virtual environment disk space, recreate venv',
    body: `
<p>A virtual environment is the most disposable directory in Python. It contains a copy of
an interpreter, a <code>site-packages</code> full of downloaded wheels, and nothing you
wrote. Recreating one is a single command. The only real question is whether that command
gives you back what you had.</p>

<h2>It depends entirely on what put the packages there</h2>

<p>If the environment was built from a lockfile, it is fully described and safe to delete:</p>

<pre><code>uv sync
poetry install
pdm sync
pipenv install --deploy</code></pre>

<p>Each reads <code>uv.lock</code>, <code>poetry.lock</code>, <code>pdm.lock</code> or
<code>Pipfile.lock</code> and rebuilds the environment to exact versions. Delete
<code>.venv</code>, run one of those, and you are where you were.</p>

<p>If the environment was built by hand — <code>python -m venv .venv</code> followed by a
few <code>pip install</code> calls over several months — then nothing describes it. There
is no file recording what is in there. Deleting it loses information, and the fact that
<code>requirements.txt</code> exists is not evidence to the contrary, because
<code>requirements.txt</code> is a wish list somebody maintains by hand and
<code>site-packages</code> is the truth.</p>

<h2>Check first: what is installed but not locked</h2>

<p>Before deleting a hand-built environment, capture what is actually in it:</p>

<pre><code>.venv/bin/python -m pip freeze &gt; requirements-frozen.txt</code></pre>

<p>On Windows: <code>.venv\\Scripts\\python -m pip freeze</code>. That file is now the
description that did not exist, and the delete is reversible.</p>

<p>For a locked project, the equivalent check is to compare the two directly. This is the
failure that bites in practice: somebody ran <code>pip install pytest-anyio</code> inside a
uv-managed environment to try something, it worked, and it never made it into
<code>uv.lock</code>. The environment now contains a package no lockfile mentions, and
<code>uv sync</code> will not bring it back.</p>

<h2>What "safe" means for the interpreter</h2>

<p>The <code>.venv</code> also contains a Python. It is a copy or a symlink to a real
interpreter installed elsewhere, and deleting the environment never touches that
interpreter. Deleting <code>.venv</code> cannot break your Python installation. It can,
however, break a <code>.venv</code> whose base interpreter has since been uninstalled — in
which case the environment was already broken and you just had not run it lately.</p>

<h2>Doing it across a machine</h2>

<p>One <code>.venv</code> is a few hundred megabytes if it has NumPy and friends in it, and
if you write Python you have dozens of them. <a href="/">dev-prune</a> handles all five
cases as separate adapters — uv, Poetry, PDM, Pipenv and plain venv — because they are
distinguished by which lockfile sits next to the environment, and it runs the matching
verification before it deletes anything:</p>

<pre><code>devp init ~/Code
devp run --dry-run</code></pre>

<p>For plain venv, where no lockfile exists at all, it compares the installed distributions
against <code>pyproject.toml</code> and refuses when it finds something it cannot account
for. A refusal there is the tool telling you the truth about a directory you were about to
lose.</p>
`,
    faq: [
      {
        q: 'Is it safe to delete a .venv folder?',
        a: 'Yes, if the environment was created from a lockfile — uv.lock, poetry.lock, pdm.lock or Pipfile.lock — because the matching sync command rebuilds it exactly. If it was created by hand with pip install over time, nothing describes its contents; run pip freeze first so the delete is reversible.',
      },
      {
        q: 'Does deleting .venv delete Python?',
        a: 'No. A virtual environment contains a copy of or a link to an interpreter installed elsewhere on the system. Deleting the environment leaves that interpreter untouched.',
      },
      {
        q: 'How do I recreate a virtual environment after deleting it?',
        a: 'uv sync, poetry install, pdm sync or pipenv install --deploy, depending on which lockfile the project uses. For a plain venv: python -m venv .venv then pip install -r requirements.txt.',
      },
    ],
    related: ['safe-to-delete-node-modules', 'reclaim-disk-space-developer-machine'],
  },

  {
    slug: 'cargo-target-directory-size',
    title: 'Why Rust’s target/ directory gets so big',
    description:
      'Multiple profiles, every dependency compiled per project, incremental artefacts and old build fingerprints — where the gigabytes go and which of them are safe to remove.',
    keywords:
      'cargo target directory size, rust target folder huge, cargo clean, reduce rust build size',
    body: `
<p>A Rust project's <code>target/</code> passing several gigabytes is not a bug and not
misconfiguration. It is four things stacked on top of one another, each individually
sensible.</p>

<h2>1. Every dependency is compiled, per project</h2>

<p>Cargo does not have a shared machine-wide compiled-artefact cache the way Maven has
<code>~/.m2</code>. <code>~/.cargo/registry</code> caches <em>source</em>; the compiled
<code>.rlib</code> files live in the project's own <code>target/</code>. Two projects
depending on the same version of the same crate compile it twice and store it twice.</p>

<h2>2. Profiles multiply everything</h2>

<p><code>target/debug</code> and <code>target/release</code> are separate full trees. Add a
test profile, a bench profile, a <code>--target</code> for cross-compilation, and each is
another one. Debug builds are the large ones, because debug info is large.</p>

<h2>3. Incremental compilation keeps state</h2>

<p><code>target/debug/incremental</code> holds the dependency graph and intermediate
products that let cargo rebuild only what changed. It grows with the number of distinct
builds you have done, and it is pure cache — deleting it costs one slower build.</p>

<h2>4. Old artefacts are never collected</h2>

<p>Cargo does not garbage-collect. Bump a dependency and the old version's compiled output
stays in <code>target/</code> forever, alongside the new one. On a long-lived project this
is often the majority of the directory.</p>

<h2>What to actually delete</h2>

<p>The blunt instrument, which is also completely safe:</p>

<pre><code>cargo clean</code></pre>

<p>Nothing in <code>target/</code> is authored by you and <code>cargo build</code>
regenerates all of it. The cost is a full cold rebuild.</p>

<p>The precise instrument, for point 4 specifically:</p>

<pre><code>cargo install cargo-sweep
cargo sweep --time 30</code></pre>

<p><code>cargo-sweep</code> removes artefacts not touched in the last 30 days while keeping
the current ones, so you get most of the space back and keep an incremental build. If you
work in one large Rust repository, this is the right tool and this article can stop here.</p>

<h2>Why it is off by default in dev-prune</h2>

<p><a href="/">dev-prune</a> has a cargo adapter, and it is one of four that ship
<strong>disabled</strong> — alongside Gradle, Maven and SwiftPM. The reason is a distinction
worth making explicit.</p>

<p>Deleting <code>node_modules</code> costs a download, and usually a download from a warm
cache. Deleting <code>target/</code> costs a <em>compile</em>, and a cold Rust compile of a
real dependency tree is minutes of CPU, not seconds of network. Both are recoverable; they
are not remotely the same price. A tool that treats them identically will eventually cost
you an afternoon in exchange for disk you were not short of.</p>

<p>So the four compiler-output adapters are opt-in, and when you enable them they get their
own idle window — longer than the one used for dependency directories, because the threshold
for "I will not miss this build" is further out than the threshold for "I will not miss this
download":</p>

<pre><code>devp config</code></pre>

<p>That opens the settings screen, where adapters are grouped by language and can be
enabled, disabled, or given a per-adapter idle threshold — individually or a whole group at
once. When cargo is enabled, the verification before deletion is
<code>cargo metadata --locked</code>, which fails if <code>Cargo.lock</code> is out of date
with respect to <code>Cargo.toml</code>. If it fails, <code>target/</code> stays.</p>
`,
    faq: [
      {
        q: 'Why is my Rust target folder so large?',
        a: 'Four reasons compound: every dependency is compiled into each project rather than shared machine-wide, debug and release are separate full trees, incremental compilation keeps its own state, and cargo never garbage-collects artefacts from dependency versions you no longer use.',
      },
      {
        q: 'Is cargo clean safe?',
        a: 'Completely. Nothing in target/ is authored by you and cargo build regenerates all of it. The only cost is a full cold rebuild the next time you compile.',
      },
      {
        q: 'What is the difference between cargo clean and cargo sweep?',
        a: 'cargo clean deletes the whole target/ directory. cargo sweep --time 30 deletes only artefacts not touched in the last 30 days, so you keep a working incremental build while reclaiming the accumulated output of old dependency versions.',
      },
    ],
    related: ['reclaim-disk-space-developer-machine', 'vs'],
  },

  {
    slug: 'vs',
    title: 'dev-prune vs kondo, npkill, cargo-sweep, rimraf and a cron job',
    description:
      'An honest comparison. Four of these are better than dev-prune at the thing they do; here is when each one is the right answer.',
    keywords:
      'kondo alternative, npkill alternative, cargo-sweep, rimraf node_modules, clean node_modules tool comparison',
    body: `
<p>Most comparison pages are written to win. This one is not, because the tools below mostly
do not overlap, and pretending they do would make it useless to the person reading it.</p>

<h2>rimraf, del-cli, rm -rf</h2>

<p><strong>Use these when you are deleting one directory you are looking at.</strong> They
are faster than anything else, they need no setup, and there is no decision to make because
you have already made it. <code>rimraf</code> exists because <code>rm -rf</code> is not
portable to Windows and Node scripts need it to be; that is its whole job and it does it.</p>

<p>Nothing below is an improvement on <code>rm -rf node_modules</code> for the single-project
case.</p>

<h2>kondo</h2>

<p><strong>Use it when you want to look at every heavy directory on the machine and decide
each one yourself, and you want that in one binary rather than one per language.</strong>
kondo is the closest thing to dev-prune that exists, it is older, it has more users, and on
the axis it was built for it is genuinely good: it walks a tree, recognises twenty-odd
project types, shows you what each one costs, and deletes what you confirm. There is a GUI
(<code>kondo-ui</code>), it is in winget, Homebrew, MacPorts and the Arch repositories, and
<code>kondo --older 30d</code> covers the common case in one line.</p>

<p>The difference is not coverage, it is what happens in the moment before a deletion.
kondo's own README says it plainly, twice:</p>

<blockquote><p>Kondo is <em>essentially</em> <code>rm -rf</code> with a prompt. Use at your
own discretion. Always have a backup of your projects.</p></blockquote>

<p>That is an accurate description and a reasonable design: you are the check, so the tool
does not need to be. dev-prune is built for the case where nobody is watching, which forces
a different set of choices — it runs <code>npm ci --dry-run</code> or
<code>uv lock --locked</code> or <code>cargo metadata --locked</code> first and keeps the
directory when that exits non-zero; it reads <code>git log</code> rather than file
timestamps, so a <code>node_modules</code> nobody has touched inside a repository you
committed to this morning is not a candidate; it records what it removed so
<code>devp restore</code> can put it back; and it schedules itself, which is the whole
point of the verification, because a background pass that guesses is a background pass that
eventually eats something.</p>

<p>Neither of those is better in the abstract. If you are going to sit there and approve
each row, kondo's warning is honest and its coverage is broad and you should use it. If you
want the machine cleaned on a timer and would rather be told “no” than be
surprised, a prompt is not the safety mechanism you need.</p>

<h2>npkill</h2>

<p><strong>Use it for a one-off sweep you want to supervise.</strong> It scans for
<code>node_modules</code>, sorts by size, shows last-modified dates, and you delete with the
arrow keys. For "I need 20&nbsp;GB back this afternoon and I want to see every row before it
goes", it is the right tool and it is pleasant to use.</p>

<p>Where it stops: it is Node-only, it is interactive by design so it does not automate, and
last-modified is a heuristic for safety rather than a check. That is a reasonable trade for
a supervised sweep — you are the check.</p>

<h2>cargo-sweep</h2>

<p><strong>Use it if your problem is one big Rust repository.</strong>
<code>cargo sweep --time 30</code> removes stale artefacts from <code>target/</code> while
leaving the current ones intact, so you reclaim most of the space <em>and</em> keep an
incremental build. Nothing else here does that; dev-prune's cargo adapter deletes
<code>target/</code> whole, which is a blunter instrument. If Rust is where your disk goes,
cargo-sweep is better at this than dev-prune is.
<a href="/cargo-target-directory-size/">Why target/ gets so big</a> has the detail.</p>

<h2>A find command in cron</h2>

<p><strong>Use it if you know exactly what you want deleted and it will not change.</strong>
Twelve lines of shell, no dependency, and it does precisely what you wrote. The failure mode
is that it does precisely what you wrote: it will delete the project you are mid-debug in,
and it will delete a <code>node_modules</code> whose lockfile no longer resolves, because
you did not write those checks and you were never going to.</p>

<h2>dev-prune</h2>

<p><strong>Use it when you want the machine to stay clean without you supervising it, and
you would rather be told "no" than be surprised.</strong> The differences that matter:</p>

<ul>
  <li><strong>It verifies before deleting.</strong> Every adapter runs the package manager's
    own dry-run — <code>npm ci --dry-run</code>, <code>pnpm install --lockfile-only</code>,
    <code>uv lock --locked</code>, <code>cargo metadata --locked</code>, <code>bundle lock
    --check</code> — and a non-zero exit means the directory stays. On the author's own
    machine, a recent pass refused 2 of 11 candidates: one repository had a webpack version
    in <code>node_modules</code> that was missing from the lockfile, and another had a
    package installed into <code>.venv</code> that <code>uv.lock</code> did not mention.
    Both would have been silently lost by any tool that deletes on a last-modified
    heuristic.</li>
  <li><strong>Twenty-three package managers, not one.</strong> npm, pnpm, yarn, bun; uv,
    Poetry, PDM, Pipenv, venv; Go, Composer, Bundler, Mix, CocoaPods, Terraform — on by
    default. Cargo, Gradle, Maven, SwiftPM, Dart, <code>_build</code> for Mix, vcpkg and
    CMake ship disabled, because their output is a compile rather than a download and
    that is a different price.</li>
  <li><strong>Idle-gated.</strong> Nothing is a candidate until its repository has gone
    without a commit or a working-tree change for a threshold you set.</li>
  <li><strong>Hard safety rules with no override flag.</strong> It never crosses a
    <code>.git</code> boundary, never follows or deletes through a symlink, and writes its
    state atomically. There is no <code>--force</code> that turns those off, deliberately.</li>
  <li><strong>Undo.</strong> <code>devp restore</code> reinstalls what the last pass removed,
    using the same lockfiles it verified.</li>
</ul>

<p>What it is not: it is not interactive scanning (npkill is better), it is not surgical
about build artefacts (cargo-sweep is better), and it is not a one-liner for a directory in
front of you (<code>rm -rf</code> is better). It is a background process that keeps a machine
from filling up, and refuses rather than guessing.</p>

<pre><code>curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
devp init ~/Code
devp run --dry-run</code></pre>

<p>Apache-2.0, a single Rust binary, no telemetry.</p>
`,
    faq: [
      {
        q: 'What is the best alternative to npkill?',
        a: 'It depends what you are doing. For a supervised one-off sweep of node_modules, npkill is very good and hard to beat. For an unsupervised, recurring cleanup across many package managers with a lockfile check before each deletion, dev-prune is built for that case. For a single directory in front of you, rm -rf is faster than both.',
      },
      {
        q: 'How does dev-prune compare to kondo?',
        a: 'kondo is the closest comparable tool and it is older and more widely packaged. It scans for heavy project directories across about twenty project types and deletes what you confirm; its own README describes it as essentially rm -rf with a prompt. dev-prune is built to run unattended instead, so it does the things a prompt would otherwise be doing: it runs the package manager dry-run that matches each lockfile and keeps the directory if that fails, gates on git activity rather than file mtime, records what it deleted so devp restore can reinstall it, and schedules itself. Supervised one-off: kondo. Unattended and recurring: dev-prune.',
      },
      {
        q: 'Does dev-prune replace cargo-sweep?',
        a: 'No. cargo-sweep removes stale artefacts from target/ while keeping the current build, which is more precise than anything dev-prune does; dev-prune deletes target/ whole and only when you have opted the cargo adapter in. If Rust build output is your main problem, use cargo-sweep.',
      },
      {
        q: 'Why not just use a cron job with find?',
        a: 'You can, and for a machine whose layout never changes it is fine. The difference is that find deletes unconditionally: it will delete a node_modules whose lockfile no longer resolves, and the project you are debugging right now, because those checks are not something a find expression can express.',
      },
    ],
    related: ['delete-node-modules-all-projects', 'cargo-target-directory-size'],
  },
];
