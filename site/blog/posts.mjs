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
export const UPDATED = '2026-09-20';

// The seven guides that shipped at the site root before everything moved under
// /blog/. Their old URLs are in a published sitemap and in links we do not control,
// so build-blog.mjs writes a redirect stub at each. A guide added after the move
// never joins this list — it has no root URL to honour.
export const LEGACY_ROOT_SLUGS = [
  'safe-to-delete-node-modules',
  'delete-node-modules-all-projects',
  'reclaim-disk-space-developer-machine',
  'clear-package-manager-cache',
  'delete-venv-safely',
  'cargo-target-directory-size',
  'vs',
];

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
anyway. <a href="/blog/delete-node-modules-all-projects/">Deleting node_modules from every
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
devp init --auto --dry-run
devp init --auto
devp run --dry-run</code></pre>

<p><code>devp init --auto</code> works out where your repositories are rather than making
you list them — it looks at the workspace you are in and the conventional code folders
under your home directory — and <code>--dry-run</code> prints what it found without
registering any of it. Name the directory yourself with <code>devp init ~/Code</code> if
you would rather. A repository holding an <code>ignore.devprune.json</code> file is left
out of the scan entirely.</p>

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
<code>deps/</code> for Mix, <code>Pods/</code> for CocoaPods: twenty-five package
managers in total, sixteen of them on by default.</p>

<h2>Which to use</h2>

<p>Clearing one project you are looking at: <code>rm -rf node_modules</code>. Nothing beats
it. Going through a machine by hand once, deciding case by case: npkill. Wanting the machine
to stay clean without you thinking about it, and wanting a refusal rather than a deletion
when something is off: that is the case dev-prune was written for. There is a fuller
comparison, including <code>cargo-sweep</code>, <code>rimraf</code> and the rest, on
<a href="/blog/vs/">dev-prune vs the alternatives</a>.</p>
`,
    faq: [
      {
        q: 'How do I delete all node_modules folders recursively?',
        a: 'On macOS or Linux: find ~/Code -name node_modules -type d -prune -exec rm -rf {} + — the -prune is what stops find from descending into directories it is about to delete. On Windows: Get-ChildItem ~/Code -Recurse -Directory -Filter node_modules | ForEach-Object { Remove-Item $_.FullName -Recurse -Force }. Neither checks whether the directories can be reinstalled.',
      },
      {
        q: 'What is the difference between npkill and dev-prune?',
        a: 'npkill scans for node_modules directories and lets you delete them interactively, sorted by size and last-modified date. dev-prune runs the package manager dry-run that matches each lockfile and deletes only when it exits zero, across twenty-five package managers rather than Node alone, and skips repositories that are not idle.',
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
rather than a guess. <a href="/blog/safe-to-delete-node-modules/">Is it safe to delete
node_modules</a> goes through those commands.</p>

<h2>2. Build directories</h2>

<p>Rust's <code>target/</code>, Gradle's <code>build/</code> and <code>.gradle/</code>,
Maven's <code>target/</code>, SwiftPM's <code>.build/</code>. Individually these are the
biggest single directories you own — a Rust workspace's <code>target/</code> passing several
gigabytes is ordinary, and <a href="/blog/cargo-target-directory-size/">there is a reason for
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
<a href="/blog/clear-package-manager-cache/">Clearing package manager caches safely</a> has the
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
<a href="/blog/cargo-target-directory-size/">Why target/ gets so big</a> has the detail.</p>

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
  <li><strong>Twenty-five package managers, not one.</strong> npm, pnpm, yarn, bun, Deno;
    uv, Poetry, PDM, Pipenv, venv; Go, Composer, Bundler, Mix, CocoaPods, Terraform, all
    on by default. Cargo, Gradle, Maven, SwiftPM, Dart, .NET, <code>_build</code> for Mix,
    vcpkg and CMake ship disabled, because their output is a compile rather than a
    download and that is a different price.</li>
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
    related: ['npkill-alternative', 'kondo-alternative', 'cargo-target-directory-size'],
  },

  {
    slug: 'npkill-alternative',
    title: 'npkill alternatives: beyond the supervised sweep',
    description:
      'npkill is good at what it does. What to reach for when you need more than node_modules, a pass that runs itself, or a check stronger than last-modified.',
    keywords:
      'npkill alternative, npkill alternatives, npkill vs kondo, npkill vs dev-prune, delete node_modules tool',
    body: `
<p><strong>If npkill is working for you, keep it.</strong> For a one-off, supervised sweep
of <code>node_modules</code> it is hard to beat: run <code>npx npkill</code>, watch it find
every install tree under the current directory, sort by size, check the last-modified
column, and delete with a keypress. Nothing below improves on that workflow for that job.</p>

<p>People search for an alternative when the job changes. It usually changes in one of
three ways, and each one points at a different tool.</p>

<h2>"My disk is not just node_modules"</h2>

<p>npkill is built around one directory name. There is a flag to point it at another
(<code>--target</code>), but the workflow stays one name at a time, and a Python
<code>.venv</code>, a Go <code>vendor/</code>, a Composer <code>vendor/</code> and an
Elixir <code>deps/</code> are four more scans you will not run.</p>

<p>Two tools cover the multi-ecosystem case. <a href="https://github.com/tbillington/kondo">kondo</a>
keeps npkill's interactive shape: it recognises twenty-odd project types in one binary,
shows what each costs, and deletes what you confirm. <a href="/">dev-prune</a> covers
twenty-five package managers, but changes the shape instead: it is built to run without
you, which is the next section.</p>

<h2>"I want this to stop being a chore"</h2>

<p>npkill is interactive by design, which means it runs exactly as often as you remember
to run it. If the real problem is that the machine fills up again every couple of months,
the fix is not a better interactive tool, it is a pass that runs on a schedule and can be
trusted to run unattended.</p>

<p>Unattended is the case dev-prune was written for, and it is why the safety model is
different: a background pass cannot ask you to eyeball a row, so every deletion is
preceded by the package manager's own check. <code>devp setup</code> registers the pass
with Task Scheduler, launchd or systemd; <code>devp run</code> only considers repositories
with no commit or working-tree change past your idle threshold; and every directory is
verified against its lockfile before it goes.</p>

<h2>"Last week I deleted something that did not come back"</h2>

<p>npkill's safety signal is the last-modified date, and the honest thing to say about
last-modified is that it measures when a file changed, not whether the directory can be
rebuilt. A <code>node_modules</code> holding a package that was installed without being
recorded in the lockfile looks identical to one that reinstalls cleanly, right up until
you delete it.</p>

<p>dev-prune runs <code>npm ci --dry-run --ignore-scripts</code> (or the pnpm, yarn or bun
equivalent) before touching anything, keeps the directory when that exits non-zero, and
records what it removed so <code>devp restore --last-run</code> can put the whole pass
back. <a href="/stress-testing-dev-prune/">Stress-testing dev-prune</a> shows the
round-trip on real installs, byte for byte.</p>

<h2>Trying it takes one dry run</h2>

<pre><code>curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
devp init ~/Code
devp run --dry-run</code></pre>

<p>The dry run deletes nothing and prints every candidate with the lockfile that proves it
rebuildable. The fuller comparison, including <code>cargo-sweep</code>,
<code>rimraf</code> and a cron job, is at
<a href="/vs/">dev-prune vs the alternatives</a>.</p>
`,
    faq: [
      {
        q: 'What is the best alternative to npkill?',
        a: 'It depends which limit you hit. For interactive cleanup across more ecosystems than Node, kondo. For an unattended, scheduled pass that verifies each directory against its lockfile before deleting and can restore what it removed, dev-prune. For one directory in front of you, rm -rf remains faster than any tool.',
      },
      {
        q: 'Can npkill run automatically on a schedule?',
        a: 'Not usefully: it is an interactive terminal UI by design, and that is a strength for the supervised case rather than a flaw. If you want scheduled cleanup, use a tool built for it. dev-prune registers a pass with Task Scheduler, launchd or systemd, gates on repository idleness, and verifies lockfiles before every deletion.',
      },
      {
        q: 'Does npkill handle Python virtualenvs or Rust target directories?',
        a: 'Its scan is built around node_modules, with a --target flag that can point it at one other directory name at a time. For .venv, vendor/, deps/ and Pods/ in one pass you want kondo (interactive) or dev-prune (unattended), both of which recognise those project types natively.',
      },
    ],
    related: ['vs', 'kondo-alternative', 'delete-node-modules-all-projects'],
  },

  {
    slug: 'kondo-alternative',
    title: 'kondo alternatives: when a prompt is not enough',
    description:
      'kondo is the broadest interactive project cleaner there is. What to use when you want the cleanup unattended, verified against lockfiles, or reversible.',
    keywords:
      'kondo alternative, kondo alternatives, kondo vs dev-prune, clean dev directories, project cleaner cli',
    body: `
<p><strong>Start by being fair to kondo:</strong> it is the broadest tool of its kind, it
is older and more widely packaged than anything below (winget, Homebrew, MacPorts, the
Arch repositories, plus a GUI in <code>kondo-ui</code>), and for "walk my disk, show me
every heavy project directory, delete what I confirm" it is genuinely good.
<code>kondo --older 30d</code> covers the common case in one line.</p>

<p>Its README describes the design honestly: "essentially rm -rf with a prompt". You are
the safety check. That is a reasonable contract, and every reason to want an alternative
is some version of wanting a different one.</p>

<h2>"I want it to run without me"</h2>

<p>A prompt only protects a deletion somebody is watching. Put kondo in a scheduled task
and you have removed the one safety mechanism it has, which its authors would be the
first to tell you not to do.</p>

<p><a href="/">dev-prune</a> is built for exactly this case, so the checks a human would
do at the prompt are done by the tool instead. It reads <code>git log</code> rather than
file timestamps, so a repository you committed to this morning is not a candidate however
old its files look. It runs the package manager's own verification, such as
<code>npm ci --dry-run</code> or <code>uv lock --locked</code> or
<code>cargo metadata --locked</code>, and a non-zero exit keeps the directory. And it
schedules itself: <code>devp setup</code> registers the pass with Task Scheduler, launchd
or systemd user timers.</p>

<h2>"I deleted something a lockfile never recorded"</h2>

<p>Age-based selection has one blind spot, and it is the expensive one: a directory
holding packages that were installed but never written to the lockfile is
indistinguishable from a clean one until it is gone. dev-prune treats the lockfile check
as the deletion criterion rather than a warning, and it refuses rather than guessing. In
<a href="/stress-testing-dev-prune/">a logged stress pass</a>, the verification and the
undo log round-tripped half a gigabyte of real installs: pruned, then restored with
<code>devp restore --last-run</code>, including a uv environment rebuilt on the recorded
Python version.</p>

<h2>"My problem is one big Rust repository"</h2>

<p>Neither kondo nor dev-prune is the best answer there.
<code>cargo sweep --time 30</code> removes stale artefacts from <code>target/</code>
while keeping the current build, which is more surgical than deleting the directory
whole. <a href="/cargo-target-directory-size/">Why target/ gets so big</a> has the
detail.</p>

<h2>"I just want to look and decide"</h2>

<p>Then you do not want an alternative: that is kondo's home ground, and for the
Node-only version of it, <a href="/npkill-alternative/">npkill</a> is also excellent.
Interactive tools are the right answer whenever you are present. The line to hold is
that a tool whose safety is a prompt should never run where nobody sees the prompt.</p>

<h2>Trying it takes one dry run</h2>

<pre><code>curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
devp init ~/Code
devp run --dry-run</code></pre>

<p>Nothing is deleted; every candidate is listed with the lockfile that proves it
rebuildable. The full comparison, including <code>rimraf</code> and a cron job, is at
<a href="/vs/">dev-prune vs the alternatives</a>.</p>
`,
    faq: [
      {
        q: 'What is the best alternative to kondo?',
        a: 'For unattended, recurring cleanup: dev-prune, which verifies each directory against its lockfile before deleting, gates on git activity rather than file age, records an undo log, and schedules itself. For supervised Node-only sweeps: npkill. For surgical Rust target/ cleanup: cargo-sweep. If you are present and deciding case by case, kondo itself remains the right tool.',
      },
      {
        q: 'Is kondo safe to use?',
        a: 'Yes, in the way its own README states: it is essentially rm -rf with a prompt, so you are the check. That is fine when you are watching. It is the wrong contract for a scheduled or scripted run, where nothing stands between an age heuristic and a deletion.',
      },
      {
        q: 'Can dev-prune replace kondo completely?',
        a: 'No. kondo covers interactive, decide-per-row cleanup across more project types than dev-prune prunes, and it has a GUI. dev-prune covers the unattended case kondo is explicitly not built for. Many machines reasonably have both installed.',
      },
    ],
    related: ['vs', 'npkill-alternative', 'reclaim-disk-space-developer-machine'],
  },

  {
    slug: 'stress-testing-dev-prune',
    title: 'Stress-testing dev-prune: every byte accounted for',
    description:
      'Six disposable repositories, real installs, one accident. What a logged prune pass deleted, what it refused, and what devp restore put back, byte for byte.',
    keywords:
      'dev-prune stress test, devp restore, lockfile verification, node_modules cleanup test, pnpm hardlinks',
    body: `
<p>Claims about deletion tools are cheap, so this is a receipt instead. We scaffolded six
disposable Git repositories on a scratch drive, gave each a real dependency install
(npm, pnpm with its store, bun, uv, Go modules with a vendor tree, cargo), registered
them, and let an AI agent drive <code>devp</code> end to end through the skill file that
<code>devp skill</code> exports. Every command was logged. The numbers below are from
those logs, not from memory.</p>

<h2>Day one: it refused to touch anything</h2>

<p>The first finding was a pass that did nothing, which is correct behaviour. Every
repository in the corpus was created that day, and <code>devp run --explain</code> gave
the same verdict for each: active, with activity today against a 15-day idle threshold.
A tool for idle repositories should treat a brand-new repository as the opposite of a
candidate. To test deletion at all we had to say so explicitly, with
<code>--ignore-idle</code>.</p>

<h2>The pass, byte for byte</h2>

<p>With the idle gate lifted, the pass verified each directory against its lockfile and
pruned six:</p>

<pre><code>  • api-server → node_modules (95.2 MiB) [npm]
  • data-pipeline → .venv (229.9 MiB) [uv]
  • go-worker → vendor (34.2 MiB) [go]
  • job-queue → node_modules (41.9 MiB) [bun]
  • web-app → node_modules (0.5 MiB) [pnpm]
  • dev-prune → site/node_modules (93.2 MiB) [npm]

  Freed: 494.99 MiB across 6 directories</code></pre>

<p>Two of those rows deserve a closer look.</p>

<p><strong>The pnpm row is small on purpose.</strong> The install was 80.9 MiB on disk,
but 80.4 MiB of it was hardlinked into the pnpm store, and deleting a hardlink does not
free the store's copy. dev-prune counts the 0.5 MiB that actually came back and reports
the hardlinked share separately, rather than taking credit for space that was never going
to be freed.</p>

<p><strong>The last row was an accident.</strong> The corpus registration swept in the
working copy of dev-prune itself, and with the idle gate explicitly lifted, the pass
verified our own site's <code>node_modules</code> against its lockfile and deleted it,
93.2 MiB, exactly as instructed. No flag saved us and none should have:
<code>--ignore-idle</code> means what it says. What made the mistake free is the next
section.</p>

<h2>The restore, byte for byte</h2>

<p><code>devp restore --last-run</code> read the undo log and reinstalled all six
directories, 494.99 MiB, from the same lockfiles the pass had verified, including
rebuilding the uv environment on the Python version the log had recorded. Our own
<code>node_modules</code> came back with everything else. A second pass over the corpus
afterwards freed 367.5 MiB across four directories, which is the first pass minus the
accident and minus the Go vendor tree, and confirmed the numbers reproduce.</p>

<h2>What it would not do</h2>

<p>The corpus included a Rust service with a 186.97 MiB <code>target/</code> directory,
and the pass left it alone. That is the opt-in policy, not a gap: <code>target/</code>
comes back by recompiling rather than downloading, which costs minutes rather than
seconds, so the cargo adapter (like Gradle, Maven, SwiftPM, .NET and the other build-tree
adapters) does nothing until you set <code>enable_cargo</code> in the config. With the
adapter enabled, the same pass claimed all 186.97 MiB.</p>

<h2>What a real machine looks like</h2>

<p>A synthetic corpus proves behaviour, not typical yield, so the number on the
<a href="/">front page</a> is a different kind: the scheduled pass of 2026-09-04 on the
author's own machine, which freed 1.91 GiB from four directories across three
repositories, each row listed with the lockfile that proved it rebuildable. Your figure
is neither of these numbers: it is what <code>devp run --dry-run</code> prints on your
disk, and that command deletes nothing.</p>

<pre><code>curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
devp init ~/Code
devp run --dry-run</code></pre>
`,
    faq: [
      {
        q: 'Does devp restore actually work?',
        a: 'In this logged pass, yes, completely: six pruned directories, 494.99 MiB, all reinstalled by devp restore --last-run from the same lockfiles the prune had verified, including a uv virtualenv rebuilt on the Python version the undo log recorded. Restore is a reinstall, so it needs network access and works precisely because nothing is pruned without a verified lockfile.',
      },
      {
        q: 'Why did dev-prune skip the Rust target directory?',
        a: 'Because build trees are opt-in. A node_modules comes back by downloading; a target/ comes back by recompiling, which is a much more expensive rebuild. The cargo, Gradle, Maven, SwiftPM and .NET adapters therefore do nothing until you enable them in the config. Once enable_cargo was set, the same pass reclaimed the full 186.97 MiB.',
      },
      {
        q: 'Are these numbers what I should expect on my machine?',
        a: 'No. The corpus was built to exercise adapters, not to look like your disk. The honest way to get your number is devp run --dry-run, which walks your registered repositories, verifies every candidate against its lockfile, prints the total, and deletes nothing.',
      },
    ],
    related: ['vs', 'safe-to-delete-node-modules', 'reclaim-disk-space-developer-machine'],
  },

  {
    slug: 'docker-disk-space',
    title: 'Where Docker disk space actually goes',
    description:
      'Images, build cache, stopped containers and volumes each cost you differently to reclaim. What each one is, which are safe to delete, and the one flag that destroys data.',
    keywords:
      'docker disk space, docker system prune, docker build cache, docker volume prune safe',
    body: `
<p><strong>Short answer: run <code>docker system df</code>.</strong> It splits what Docker
holds into four rows, and the four are not equally safe to reclaim: images, containers,
local volumes and build cache. Three of them come back by pulling or rebuilding. The
fourth is the only copy of whatever is inside it.</p>

<h2>The four rows, safest first</h2>

<ul>
  <li><strong>Build cache.</strong> Layers BuildKit kept so your next build starts warm.
    Deleting it costs exactly one cold build per project. <code>docker builder prune -a -f</code>
    clears it.</li>
  <li><strong>Unused images.</strong> Base images and old tags nothing references. They come
    back from the registry when something pulls them, at the cost of the download.
    <code>docker image prune -a -f</code> takes every image no container uses.</li>
  <li><strong>Stopped containers.</strong> A stopped container keeps its writable layer.
    Anything a process wrote inside the container, and not into a mounted volume, lives in
    that layer and goes with it. If that describes data you care about, it was in the wrong
    place, and the time to move it is before the prune. <code>docker container prune -f</code>
    removes stopped containers.</li>
  <li><strong>Volumes.</strong> A named volume is where databases, message queues and
    anything else with real state keep it. There is no registry behind a volume and no
    rebuild that brings one back. This row is not a cache and should never be cleared like
    one.</li>
</ul>

<h2>The flag that turns cleanup into data loss</h2>

<p><code>docker system prune --volumes</code> deletes every volume no container currently
references. "Currently" is the trap: a database whose container is stopped, or removed and
recreated on demand by compose, counts as unreferenced at that moment. People run the flag
to reclaim cache space and delete a local database as a side effect. If you want volume
space back, list them with <code>docker volume ls</code>, look at what each one is, and
remove the ones you can name with <code>docker volume rm</code>, one at a time.</p>

<h2>Why the numbers look strange</h2>

<p>Two things about Docker's accounting are worth knowing before you compare numbers.
Layers are shared, so deleting three images can free far less than the sum of their listed
sizes. And on Docker Desktop the whole store lives inside a virtual machine disk the host
cannot see into, so the honest measurement is the engine's own <code>system df</code>,
before and after, rather than anything a directory walk on the host can tell you.</p>

<h2>Doing it with a tool that keeps score</h2>

<p><a href="/">dev-prune</a> treats container disk as something you look at often and
delete from deliberately. <code>devp caches docker</code> prints the four rows, sized, with
how much the engine says is reclaimable, and deletes nothing. <code>devp caches clear
docker</code> runs the three narrow commands above (build cache, unused images, stopped
containers), prints them before running anything, asks, and measures what came back by
asking the engine again afterwards, so the figure lands in <code>devp stats</code> instead
of being forgotten. Podman, nerdctl, finch and Apple's container engine each get the
same report and their own engine's equivalent of those narrow commands.</p>

<p>Volumes are excluded from that estimate and from those commands: none of the engine
commands dev-prune runs contains the word "volume", and a test fails the build if one
appears. The one path that touches them, <code>devp caches clear docker
--include-volumes</code>, lists the unused volumes by name and takes each deletion as a
typed pick at a real terminal, one unforced <code>docker volume rm</code> per pick. It
refuses <code>--yes</code>, <code>--json</code> and piped input, so no script, scheduler
or AI agent can reach the picking. And typed cold, it does not delete at all: the pick
list arms only within ten minutes of a completed dry run for that engine, so the real
command runs the dry run instead and says so. Nothing bulk, nothing silent.</p>
`,
    faq: [
      {
        q: 'Is docker system prune safe to run?',
        a: 'Without --volumes, mostly: it removes stopped containers, unused networks, dangling images and dangling build cache, all of which come back by pulling or rebuilding. The caveat is stopped containers, whose writable layers go with them, so anything a process wrote inside a container rather than into a volume is lost. With --volumes it stops being a cleanup command: it deletes every volume no container currently references, which includes the database whose container happens to be stopped.',
      },
      {
        q: 'What is using all my Docker disk space?',
        a: 'Run docker system df for the split across images, containers, volumes and build cache, or devp caches docker for the same figures with per-row reclaimable amounts. On build machines the build cache is usually the biggest recoverable row; on machines running databases in containers, volumes often dominate and should be left alone.',
      },
      {
        q: 'How do I delete Docker volumes safely?',
        a: 'By name, one at a time, after looking: docker volume ls, then docker volume rm for the ones you can identify. Avoid docker volume prune and docker system prune --volumes, which delete by reference counting rather than by your judgment. devp caches clear docker --include-volumes wraps the same per-name deletion in a typed pick list that scripts and agents cannot answer.',
      },
    ],
    related: ['clear-package-manager-cache', 'reclaim-disk-space-developer-machine'],
  },

  {
    slug: 'ai-agents-disk-cleanup',
    title: 'Giving an AI agent a safe way to free disk space',
    description:
      'An agent that runs rm -rf has no dry run, no proof and no undo. What to demand of any cleanup an agent performs, and how to teach yours the safer path.',
    keywords:
      'ai coding agent cleanup, agent rm -rf node_modules, claude code disk space, cursor rules cleanup',
    body: `
<p>Coding agents notice full disks. They see the failing write, they know
<code>node_modules</code> is rebuildable, and the shortest path from problem to fix is
<code>rm -rf</code>. Sometimes that is fine. The times it is not fine are the times a
lockfile no longer resolved, or the directory held something edited in place, or the
command was <code>docker system prune --volumes</code> and the thing reclaimed was a
database. An agent deleting by hand has no dry run, no proof the thing comes back, and no
record to undo from.</p>

<h2>What to demand of any deletion an agent performs</h2>

<p>The bar is the same whether the agent is a person or a model, but a model needs it
written down:</p>

<ul>
  <li><strong>A dry run first.</strong> The plan is shown before anything is deleted, and
    the real run does exactly what the plan said.</li>
  <li><strong>Proof of recoverability.</strong> Not "this is usually rebuildable" but a
    check, run now, that this particular directory rebuilds from what sits beside it.</li>
  <li><strong>A record and an undo.</strong> Every deletion is written down, and one
    command puts it back.</li>
  <li><strong>Refusals that cannot be scripted around.</strong> The dangerous paths refuse
    piped input and auto-confirm flags, so the agent can prepare the command but only a
    person can run it.</li>
</ul>

<p><a href="/">dev-prune</a> is those four bullets as a binary. <code>devp run
--dry-run</code> is the plan; lockfile verification runs before every deletion and has no
bypass flag; <code>devp history</code> records each pass and what started it; <code>devp
restore --last-run</code> reinstalls exactly what the last pass removed. The one path that
can touch a container volume takes each deletion as a typed pick and refuses
<code>--yes</code>, <code>--json</code> and piped stdin, so an agent can run the
<code>--dry-run</code> form and hand the final command to you, and nothing more.</p>

<h2>Teaching the agent it exists</h2>

<p>An agent uses the safer path only if it knows the path is there. <code>devp skill</code>
handles that: on its own it reports which coding tools this machine or repository shows
traces of (pure existence checks, nothing executed), and with <code>--agent</code> or
<code>--detected</code> it writes a rules file into the current repository in the place
each editor's agent actually reads: <code>.cursor/rules/</code>
for Cursor, <code>.windsurf/rules/</code> for Windsurf, a marked block in
<code>AGENTS.md</code> for the tools that read the shared convention, and a dozen others.</p>

<pre><code>devp skill                    # what is detected, and what is current or missing
devp skill --agent cursor     # rules for one editor
devp skill --detected         # rules for every detected editor at once</code></pre>

<p>The rules are inert text, safe to commit, and they say the things above in the agent's
terms: never rm -rf a bloat directory by hand, restore through devp restore rather than
reinstalling manually, never touch a volume, never empty the Maven local repository
uninvited. Claude Code is the one tool not written per repository, because its skill
installs globally and every project gets it.</p>

<h2>When rules are not enough</h2>

<p>Rules are advisory: a long session can bury them. Harnesses with command hooks can turn
the two rules that matter most into a real confirmation prompt, so a volume deletion the
agent composes stops and asks you first. A copy-paste hook for Claude Code, with the
matching patterns and the reasoning, lives in the
<a href="https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/IDE_INTEGRATION.md">IDE
integration guide</a>.</p>
`,
    faq: [
      {
        q: 'How do I stop my AI agent deleting node_modules by hand?',
        a: 'Give it a better path and write the rule where it reads. devp skill --detected writes a rules file for every coding tool your repository or machine shows traces of, and the rules say to prune through devp run (which verifies the lockfile first and records the deletion) and to put things back with devp restore. For a harness with command hooks, the IDE integration guide has a hook that turns the dangerous commands into a confirmation prompt.',
      },
      {
        q: 'What if the agent already deleted a dependencies directory?',
        a: 'If dev-prune deleted it, devp restore reinstalls it, and devp history shows which pass took it and what started that pass. If the agent ran rm -rf itself, there is no record to restore from: reinstall from the lockfile with your package manager, and check the lockfile still resolves before trusting the result.',
      },
      {
        q: 'Can an AI agent delete Docker volumes through dev-prune?',
        a: 'No. The only command that touches volumes lists them by name and takes each deletion as a typed pick at an interactive terminal; it refuses --yes, --json and piped stdin, which are the three ways an agent answers prompts. The agent can run the --dry-run form, which lists the unused volumes and prints the command for a person to run, and that is the intended division of labour.',
      },
    ],
    related: ['safe-to-delete-node-modules', 'docker-disk-space'],
  },

  {
    slug: 'gradle-maven-build-directories',
    title: 'Cleaning Gradle and Maven build directories',
    description:
      'target/, build/ and .gradle/ rebuild from the project beside them. The local repository at ~/.m2 does not, and treating it as a cache is how artifacts vanish.',
    keywords:
      'delete maven target folder, gradle build directory, clear gradle cache, is m2 repository safe to delete',
    body: `
<p><strong>Short answer: the per-project build trees are safe to delete.</strong> Maven's
<code>target/</code> comes back on the next <code>mvn package</code>. Gradle's
<code>build/</code> and <code>.gradle/</code> come back on the next build. sbt's
<code>target/</code> and <code>project/target/</code> likewise. Everything in them is
compiled from the sources and the build file sitting beside them, and on a JVM project of
any age they are usually the largest directories in the repository.</p>

<p>What they cost to delete is not download time but compile time. A
<code>node_modules</code> returns as fast as the network allows; a build tree returns as
fast as your machine compiles, which on a large project is the difference between seconds
and minutes. That distinction matters later.</p>

<h2>The machine-wide stores are a different question</h2>

<p>Outside the repository, both tools keep shared state under your home directory, and the
two are not equally safe.</p>

<p><strong>Gradle's is a cache.</strong> <code>~/.gradle/caches</code> holds downloaded
dependencies and build metadata; <code>~/.gradle/wrapper/dists</code> holds the Gradle
distributions your wrappers have fetched. Both are re-downloaded on demand. Deleting them
costs every Gradle project on the machine one cold start, and nothing else.</p>

<p><strong>Maven's is not.</strong> <code>~/.m2/repository</code> is called the local
repository, not the cache, and the name is load-bearing. <code>mvn install</code> writes
your own builds into it. <code>mvn install:install-file</code> is the documented way to
use a jar that exists in no remote repository at all, which is how a database driver
behind a click-through licence or a partner SDK ends up there, with no origin to fetch it
back from. Your <code>-SNAPSHOT</code> builds live there and nowhere else. Emptying it as
if it were a download cache deletes the recoverable majority and the unrecoverable
minority together, and you find out which was which at the next build failure.</p>

<h2>How dev-prune handles the pair</h2>

<p>In <a href="/">dev-prune</a>, Gradle and Maven are opt-in adapters, off by default like
every adapter whose directories come back by recompiling rather than downloading. Turning
one on is one setting:</p>

<pre><code>devp config set enable_gradle true
devp config set enable_maven true</code></pre>

<p>An enabled build adapter waits for the longer idle window (45 days by default, against
the usual 15) before its directories become candidates, so a project you build weekly
keeps its warm build tree and a repository untouched since spring gives its tree up. Every
directory is still verified against the project file beside it before deletion, and a dry
run of <code>devp run</code> ends by naming any build adapters that are switched off but
would have found something.</p>

<p>The machine-wide stores follow the split above. <code>devp caches</code> sizes both.
<code>devp caches clear gradle</code> empties Gradle's caches and wrapper distributions,
after showing what would go and asking. <code>devp caches clear maven</code> is refused
outright: it exits with a usage error, explains that the local repository holds artifacts
no remote can restore, and prints the <code>rm -rf</code> line so that if you want that
space, the decision and the keystroke are both yours.</p>
`,
    faq: [
      {
        q: 'Is it safe to delete the target folder in a Maven project?',
        a: 'Yes. target/ holds compiled classes, packaged jars and generated sources, all rebuilt by the next mvn package from pom.xml and your sources. mvn clean deletes it through Maven; deleting the directory directly has the same effect. The cost is the recompile, not any lost data.',
      },
      {
        q: 'Is it safe to delete ~/.gradle/caches?',
        a: 'Yes. It holds downloaded dependencies and build metadata that Gradle re-fetches on demand, and ~/.gradle/wrapper/dists beside it holds re-downloadable Gradle distributions. The cost is one cold build per project on the machine while the cache refills.',
      },
      {
        q: 'Is it safe to delete the ~/.m2 repository?',
        a: 'Not as a bulk operation. Most of it is re-downloadable, but mvn install and mvn install:install-file write artifacts there that exist in no remote repository: your own SNAPSHOT builds, and third-party jars installed by hand. Maven records artifact origins only in an internal file it documents as free to change, so no tool can reliably separate the recoverable part from the rest. Delete specific subdirectories you can vouch for, or accept that emptying it may cost artifacts you cannot re-fetch.',
      },
    ],
    related: ['cargo-target-directory-size', 'clear-package-manager-cache'],
  },
];
