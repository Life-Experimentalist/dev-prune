// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Static guide pages.
//
// `npm run build` runs this last, after the client build, the SSR build and
// `prerender.js`. It emits one directory per entry in `blog/posts.mjs`, plus an index
// at /blog/, plus a sitemap covering all of them.
//
// Why not React and a router? Because these pages are documents. They have no state,
// nothing to hydrate, and the whole point of them is to be the fastest possible thing a
// crawler can read. Rendering them as strings at build time means they ship as HTML with
// one stylesheet and no JavaScript at all — the theme script and nothing else. Adding a
// router to the app would have meant a second entry point, a second prerender pass and a
// hydration boundary, in exchange for nothing a reader would notice.
//
// The stylesheet is the one the app already emitted: this reads its hashed filename out
// of `dist/index.html` rather than guessing it, so a cache-busting rebuild cannot leave
// the guides pointing at a file that no longer exists.

import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { POSTS, SITE, UPDATED } from './blog/posts.mjs';
import { REFERENCE, referenceMain } from './reference.mjs';

const root = dirname(fileURLToPath(import.meta.url));
const dist = resolve(root, 'dist');
const indexPath = resolve(dist, 'index.html');

if (!existsSync(indexPath)) {
  console.error('build-blog: dist/index.html is missing — run the site build first.');
  process.exit(1);
}

const shell = readFileSync(indexPath, 'utf8');

const cssMatch = shell.match(/<link rel="stylesheet"[^>]*href="([^"]+\.css)"[^>]*>/);
if (!cssMatch) {
  console.error('build-blog: no stylesheet link found in dist/index.html.');
  process.exit(1);
}
const CSS = cssMatch[1];

// The pre-paint theme resolver, lifted verbatim from index.html. Copying it rather than
// importing it keeps these pages dependency-free; if it drifts, the guides fall back to
// the dark default, which is the same thing a visitor with JS off already gets.
const themeScript = shell.match(
  /<script>\s*\(function \(\) \{\s*var root = document\.documentElement;[\s\S]*?\}\)\(\);\s*<\/script>/,
);
if (!themeScript) {
  console.error('build-blog: could not find the theme script in dist/index.html.');
  process.exit(1);
}

const esc = (s) =>
  s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');

const bySlug = new Map(POSTS.map((p) => [p.slug, p]));

function page({ url, title, description, keywords, main, jsonLd }) {
  return `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta name="theme-color" content="#0B0F17" />

    <title>${esc(title)} — dev-prune</title>
    <meta name="description" content="${esc(description)}" />
${keywords ? `    <meta name="keywords" content="${esc(keywords)}" />\n` : ''}    <meta name="author" content="VKrishna04" />
    <meta name="robots" content="index, follow, max-image-preview:large, max-snippet:-1" />
    <link rel="canonical" href="${SITE}${url}" />

    <meta property="og:type" content="article" />
    <meta property="og:site_name" content="dev-prune" />
    <meta property="og:url" content="${SITE}${url}" />
    <meta property="og:title" content="${esc(title)}" />
    <meta property="og:description" content="${esc(description)}" />
    <meta property="og:image" content="${SITE}/assets/og-card.jpg" />
    <meta property="og:image:type" content="image/jpeg" />
    <meta property="og:image:width" content="1200" />
    <meta property="og:image:height" content="630" />
    <meta name="twitter:card" content="summary_large_image" />
    <meta name="twitter:title" content="${esc(title)}" />
    <meta name="twitter:description" content="${esc(description)}" />
    <meta name="twitter:image" content="${SITE}/assets/og-card.jpg" />

    <link rel="icon" type="image/png" href="/assets/favicon/favicon-96x96.png" sizes="96x96" />
    <link rel="icon" href="/assets/favicon/favicon.ico" sizes="any" />
    <link rel="apple-touch-icon" sizes="180x180" href="/assets/favicon/apple-touch-icon.png" />
    <link rel="manifest" href="/assets/favicon/site.webmanifest" />
    <link rel="alternate" type="text/plain" href="/llms.txt" title="llms.txt" />

    <link rel="stylesheet" href="${CSS}" />
    <script type="application/ld+json">
${JSON.stringify(jsonLd, null, 2)}
    </script>
    ${themeScript[0]}
  </head>
  <body>
    <div class="app doc-page">
      <a href="#main" class="skip-link">Skip to content</a>
      <div class="bg-glow bg-glow-1" aria-hidden="true"></div>
      <div class="bg-glow bg-glow-2" aria-hidden="true"></div>

      <header class="navbar">
        <div class="container nav-content">
          <a href="/" class="brand">
            <div class="logo-box">
              <img src="/assets/icon_small.png" alt="" width="32" height="32" class="logo-icon-img" />
            </div>
            <span class="brand-name">dev-prune</span>
          </a>
          <nav class="nav-links" aria-label="Primary">
            <a href="/blog/">Guides</a>
            <a href="/reference/">Reference</a>
            <a href="/#install">Install</a>
            <a href="/#faq">FAQ</a>
            <a href="https://github.com/Life-Experimentalist/dev-prune" class="btn btn-secondary nav-github" target="_blank" rel="noreferrer">GitHub</a>
          </nav>
        </div>
      </header>

      <main id="main" class="doc-main">
${main}
      </main>

      <footer class="footer">
        <div class="container footer-content">
          <div class="footer-brand">
            <img src="/assets/icon_small.png" alt="" width="24" height="24" />
            <span>dev-prune (devp)</span>
            <span class="footer-tagline">· lockfile-safe workspace pruner</span>
          </div>
          <nav class="footer-links" aria-label="Footer">
            <a href="/">Home</a>
            <a href="/blog/">Guides</a>
            <a href="/reference/">Reference</a>
            <a href="https://github.com/Life-Experimentalist/dev-prune" target="_blank" rel="noreferrer">GitHub</a>
            <a href="https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/README.md" target="_blank" rel="noreferrer">Docs</a>
            <a href="https://vkrishna04.me" target="_blank" rel="noreferrer">vkrishna04.me</a>
          </nav>
        </div>
        <div class="container">
          <p class="footer-legal">
            Copyright 2026 <a href="https://vkrishna04.me" target="_blank" rel="noreferrer">VKrishna04</a>
            · Licensed under the Apache License, Version 2.0
          </p>
        </div>
      </footer>
    </div>
  </body>
</html>
`;
}

function articleJsonLd(post) {
  const url = `${SITE}/${post.slug}/`;
  const graph = [
    {
      '@type': 'TechArticle',
      '@id': `${url}#article`,
      headline: post.title,
      description: post.description,
      inLanguage: 'en',
      datePublished: UPDATED,
      dateModified: UPDATED,
      mainEntityOfPage: url,
      author: { '@type': 'Person', name: 'VKrishna04', url: 'https://vkrishna04.me' },
      publisher: { '@type': 'Organization', name: 'dev-prune', url: SITE },
    },
    {
      '@type': 'BreadcrumbList',
      itemListElement: [
        { '@type': 'ListItem', position: 1, name: 'dev-prune', item: `${SITE}/` },
        { '@type': 'ListItem', position: 2, name: 'Guides', item: `${SITE}/blog/` },
        { '@type': 'ListItem', position: 3, name: post.title, item: url },
      ],
    },
  ];
  if (post.faq?.length) {
    graph.push({
      '@type': 'FAQPage',
      '@id': `${url}#faq`,
      mainEntity: post.faq.map((f) => ({
        '@type': 'Question',
        name: f.q,
        acceptedAnswer: { '@type': 'Answer', text: f.a },
      })),
    });
  }
  return { '@context': 'https://schema.org', '@graph': graph };
}

function articleMain(post) {
  const faq = post.faq?.length
    ? `        <section class="doc-faq" aria-labelledby="faq-h">
          <h2 id="faq-h">Common questions</h2>
${post.faq
  .map(
    (f) => `          <details class="faq-item">
            <summary><span>${esc(f.q)}</span></summary>
            <div class="faq-answer"><p>${esc(f.a)}</p></div>
          </details>`,
  )
  .join('\n')}
        </section>
`
    : '';

  const related = post.related
    ?.map((slug) => bySlug.get(slug))
    .filter(Boolean)
    .map(
      (r) => `            <li><a href="/${r.slug}/"><strong>${esc(r.title)}</strong><span>${esc(
        r.description,
      )}</span></a></li>`,
    )
    .join('\n');

  return `        <article class="doc-article container narrow">
          <nav class="doc-crumbs" aria-label="Breadcrumb">
            <a href="/">dev-prune</a> <span aria-hidden="true">/</span> <a href="/blog/">Guides</a>
          </nav>
          <h1>${esc(post.title)}</h1>
          <p class="doc-lede">${esc(post.description)}</p>
          <p class="doc-meta">Updated ${UPDATED}</p>
${post.body.trim().split('\n').map((l) => (l ? '          ' + l : l)).join('\n')}
${faq}${
    related
      ? `        <section class="doc-related" aria-labelledby="rel-h">
          <h2 id="rel-h">Keep reading</h2>
          <ul>
${related}
          </ul>
        </section>
`
      : ''
  }        </article>
`;
}

function indexMain() {
  return `        <div class="doc-article container narrow">
          <nav class="doc-crumbs" aria-label="Breadcrumb">
            <a href="/">dev-prune</a> <span aria-hidden="true">/</span> Guides
          </nav>
          <h1>Guides</h1>
          <p class="doc-lede">
            Straight answers to the questions people ask when a disk fills up. Every one of
            these is written to be useful whether or not you ever install anything.
          </p>
          <section class="doc-related">
            <ul>
${POSTS.map(
  (p) => `              <li><a href="/${p.slug}/"><strong>${esc(p.title)}</strong><span>${esc(
    p.description,
  )}</span></a></li>`,
).join('\n')}
            </ul>
          </section>
        </div>
`;
}

function write(dir, html) {
  const target = resolve(dist, dir);
  mkdirSync(target, { recursive: true });
  writeFileSync(resolve(target, 'index.html'), html, 'utf8');
}

for (const post of POSTS) {
  write(
    post.slug,
    page({
      url: `/${post.slug}/`,
      title: post.title,
      description: post.description,
      keywords: post.keywords,
      main: articleMain(post),
      jsonLd: articleJsonLd(post),
    }),
  );
}

write(
  'blog',
  page({
    url: '/blog/',
    title: 'Guides',
    description:
      'Guides on reclaiming disk space from a developer machine: node_modules, .venv, build directories and package manager caches.',
    keywords: 'node_modules, venv, disk space, developer machine cleanup',
    main: indexMain(),
    jsonLd: {
      '@context': 'https://schema.org',
      '@graph': [
        {
          '@type': 'CollectionPage',
          '@id': `${SITE}/blog/#page`,
          name: 'dev-prune guides',
          url: `${SITE}/blog/`,
          hasPart: POSTS.map((p) => ({
            '@type': 'TechArticle',
            headline: p.title,
            description: p.description,
            url: `${SITE}/${p.slug}/`,
          })),
        },
      ],
    },
  }),
);

// The CLI reference, rendered from the document that ships with the source rather than
// written a second time here. `reference.mjs` throws on anything it cannot render and on
// any intra-document anchor that does not resolve, so a broken reference fails the build.
write(
  'reference',
  page({
    url: REFERENCE.url,
    title: REFERENCE.title,
    description: REFERENCE.description,
    keywords: REFERENCE.keywords,
    main: referenceMain(resolve(root, '..')),
    jsonLd: {
      '@context': 'https://schema.org',
      '@graph': [
        {
          '@type': 'TechArticle',
          '@id': `${SITE}${REFERENCE.url}#article`,
          headline: `dev-prune ${REFERENCE.title}`,
          description: REFERENCE.description,
          inLanguage: 'en',
          datePublished: UPDATED,
          dateModified: UPDATED,
          mainEntityOfPage: `${SITE}${REFERENCE.url}`,
          author: { '@type': 'Person', name: 'VKrishna04', url: 'https://vkrishna04.me' },
          publisher: { '@type': 'Organization', name: 'dev-prune', url: SITE },
        },
        {
          '@type': 'BreadcrumbList',
          itemListElement: [
            { '@type': 'ListItem', position: 1, name: 'dev-prune', item: `${SITE}/` },
            {
              '@type': 'ListItem',
              position: 2,
              name: REFERENCE.title,
              item: `${SITE}${REFERENCE.url}`,
            },
          ],
        },
      ],
    },
  }),
);

// The sitemap is generated rather than hand-maintained, because a guide added to
// `posts.mjs` and forgotten in an XML file is a page that never gets crawled.
const urls = [
  { loc: `${SITE}/`, priority: '1.0', changefreq: 'weekly' },
  { loc: `${SITE}/blog/`, priority: '0.8', changefreq: 'weekly' },
  { loc: `${SITE}${REFERENCE.url}`, priority: '0.9', changefreq: 'weekly' },
  ...POSTS.map((p) => ({
    loc: `${SITE}/${p.slug}/`,
    priority: '0.7',
    changefreq: 'monthly',
  })),
  { loc: `${SITE}/llms.txt`, priority: '0.5', changefreq: 'monthly' },
];

writeFileSync(
  resolve(dist, 'sitemap.xml'),
  `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${urls
  .map(
    (u) => `  <url>
    <loc>${u.loc}</loc>
    <lastmod>${UPDATED}</lastmod>
    <changefreq>${u.changefreq}</changefreq>
    <priority>${u.priority}</priority>
  </url>`,
  )
  .join('\n')}
</urlset>
`,
  'utf8',
);

console.log(
  `build-blog: wrote ${POSTS.length} guides, /blog/, ${REFERENCE.url} and a ${urls.length}-URL sitemap.`,
);
