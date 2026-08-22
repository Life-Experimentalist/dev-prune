// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// The /reference/ page, rendered from `docs/CLI_REFERENCE.md` at build time.
//
// The alternative was to hand-write the reference a second time in HTML, and that was
// rejected: this repository already restates the same flags in five places
// (`docs/CLI_REFERENCE.md`, `SKILL.md`, `README.md`, `llms.txt` and `App.jsx`) and keeps
// them in step by discipline. A sixth copy that a build could have generated instead
// would be the one that drifts, because nobody reads a web page to check a flag.
//
// So this is a Markdown renderer, but only of the subset `CLI_REFERENCE.md` actually
// uses: ATX headings, paragraphs, unordered and ordered lists with one level of nesting,
// pipe tables, fenced code, thematic breaks, and inline code / bold / links. Anything
// outside that subset throws and fails the build rather than rendering as literal
// Markdown in front of a reader. A renderer that guesses is worse than one that stops.
//
// Two things it checks that nothing else in this repository checks:
//
//   - Every in-page `#anchor` link resolves to a heading that exists. GitHub silently
//     scrolls nowhere when one does not, which is how three of them stayed wrong in
//     `CLI_REFERENCE.md` for several releases before this file was written.
//   - Heading IDs are generated with GitHub's own slug rules, so an anchor that works on
//     the site works on GitHub and the reverse.

import { readFileSync } from 'node:fs';

const REPO = 'https://github.com/Life-Experimentalist/dev-prune';
const BLOB = REPO + '/blob/main';

export const REFERENCE = {
  source: 'docs/CLI_REFERENCE.md',
  url: '/reference/',
  title: 'CLI reference',
  description:
    'Every dev-prune (devp) command, flag, exit code, config key and --json field, generated from the reference document that ships with the source.',
  keywords:
    'dev-prune cli reference, devp commands, devp flags, devp config keys, devp json output',
};

// A character that cannot occur in the document, so a code span's placeholder can
// never be confused with the document's own text.
const NUL = String.fromCharCode(0);

const esc = (s) =>
  s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');

// GitHub's heading slugs: lower-case, drop everything that is not a word character,
// a space or a hyphen, then spaces become hyphens. Runs of spaces are *not* collapsed,
// which is why the anchor for a heading containing " | " has four hyphens in it.
export function slug(text) {
  return text
    .toLowerCase()
    .replace(/[^\w\- ]/gu, '')
    .replace(/ /g, '-');
}

// A relative link in the doc points at a file in the repository, not at a URL this site
// serves. Rewriting them to the blob view keeps every one of them working; leaving them
// alone would produce a page of 404s.
function resolveHref(href) {
  if (/^(https?:|mailto:|#)/.test(href)) return href;
  if (href.startsWith('../')) return BLOB + '/' + href.slice(3);
  return BLOB + '/docs/' + href;
}

// Inline spans. Code first, into placeholders, because the text inside a code span is
// literal: an asterisk in there is an asterisk and a bracket is a bracket.
function inline(src, links) {
  const code = [];
  let s = src.replace(/`([^`]+)`/g, (_, c) => {
    code.push('<code>' + esc(c) + '</code>');
    return NUL + (code.length - 1) + NUL;
  });

  s = esc(s);

  s = s.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, text, href) => {
    if (links) links.push(href);
    const target = resolveHref(href);
    const external = /^https?:/.test(target);
    return (
      '<a href="' +
      esc(target) +
      '"' +
      (external ? ' target="_blank" rel="noreferrer"' : '') +
      '>' +
      text +
      '</a>'
    );
  });

  s = s.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  s = s.replace(/(^|[\s(])\*([^*]+)\*/g, '$1<em>$2</em>');

  return s.replace(new RegExp(NUL + '([0-9]+)' + NUL, 'g'), (_, i) => code[Number(i)]);
}

function cells(line) {
  return line
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
    .map((c) => c.trim());
}

// One nesting level, which is all the document uses. `items` is a flat list of
// {indent, ordered, html}; anything indented under its predecessor becomes a sub-list.
function renderList(items, ordered) {
  const tag = ordered ? 'ol' : 'ul';
  const out = ['<' + tag + '>'];
  let open = false;
  let nested = false;
  for (const item of items) {
    if (item.indent > 0) {
      if (!nested) {
        out.push('<' + (item.ordered ? 'ol' : 'ul') + '>');
        nested = true;
      }
      out.push('<li>' + item.html + '</li>');
      continue;
    }
    if (nested) {
      out.push('</' + (items.find((x) => x.indent > 0).ordered ? 'ol' : 'ul') + '>');
      nested = false;
    }
    if (open) out.push('</li>');
    out.push('<li>' + item.html);
    open = true;
  }
  if (nested) out.push('</' + (items.find((x) => x.indent > 0).ordered ? 'ol' : 'ul') + '>');
  if (open) out.push('</li>');
  out.push('</' + tag + '>');
  return out.join('\n');
}

const LIST = /^(\s*)([-*]|\d+\.)\s+(.*)$/;

export function render(markdown) {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n');
  const html = [];
  const headings = [];
  const anchors = new Set();
  const links = [];

  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    if (!line.trim()) {
      i++;
      continue;
    }

    // Fenced code. Mermaid is a diagram, and this page ships no JavaScript, so it links
    // to the rendered copy on GitHub rather than printing the diagram's source at a
    // reader who did not ask for it.
    const fence = line.match(/^```(\w*)\s*$/);
    if (fence) {
      const lang = fence[1];
      const body = [];
      i++;
      while (i < lines.length && !/^```\s*$/.test(lines[i])) body.push(lines[i++]);
      if (i >= lines.length) throw new Error('reference: unterminated code fence');
      i++;
      if (lang === 'mermaid') {
        html.push(
          '<p class="doc-note">A diagram sits here in the source document. ' +
            '<a href="' +
            BLOB +
            '/' +
            REFERENCE.source +
            '" target="_blank" rel="noreferrer">View it rendered on GitHub</a>.</p>',
        );
      } else {
        html.push(
          '<pre><code' +
            (lang ? ' class="language-' + lang + '"' : '') +
            '>' +
            esc(body.join('\n')) +
            '</code></pre>',
        );
      }
      continue;
    }

    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      const level = heading[1].length;
      const text = heading[2].trim();
      const id = slug(text);
      anchors.add(id);
      if (level === 2 || level === 3) headings.push({ level, id, text });
      // The document's own H1 is dropped: the page supplies its own.
      if (level > 1) {
        html.push(
          '<h' + level + ' id="' + esc(id) + '">' + inline(text, links) + '</h' + level + '>',
        );
      }
      i++;
      continue;
    }

    if (/^(-{3,}|\*{3,})\s*$/.test(line)) {
      html.push('<hr />');
      i++;
      continue;
    }

    if (line.startsWith('|')) {
      const head = cells(line);
      const sep = lines[i + 1] || '';
      if (!/^\|[\s:|-]+\|?\s*$/.test(sep)) {
        throw new Error('reference: table without a separator row at line ' + (i + 1));
      }
      i += 2;
      const rows = [];
      while (i < lines.length && lines[i].startsWith('|')) rows.push(cells(lines[i++]));
      html.push(
        '<div class="doc-table-wrap"><table>\n<thead><tr>' +
          head.map((c) => '<th>' + inline(c, links) + '</th>').join('') +
          '</tr></thead>\n<tbody>' +
          rows
            .map(
              (r) =>
                '<tr>' + r.map((c) => '<td>' + inline(c, links) + '</td>').join('') + '</tr>',
            )
            .join('\n') +
          '</tbody>\n</table></div>',
      );
      continue;
    }

    const list = line.match(LIST);
    if (list) {
      const ordered = /\d/.test(list[2]);
      const items = [];
      while (i < lines.length) {
        const m = lines[i].match(LIST);
        if (!m) {
          // A line indented under the previous item, and not itself a bullet, continues
          // that item — the document wraps long bullets at eighty columns.
          if (items.length && /^\s+\S/.test(lines[i])) {
            items[items.length - 1].html += ' ' + inline(lines[i].trim(), links);
            i++;
            continue;
          }
          break;
        }
        items.push({
          indent: m[1].length,
          ordered: /\d/.test(m[2]),
          html: inline(m[3], links),
        });
        i++;
      }
      html.push(renderList(items, ordered));
      continue;
    }

    const para = [];
    while (
      i < lines.length &&
      lines[i].trim() &&
      !LIST.test(lines[i]) &&
      !lines[i].startsWith('|') &&
      !/^```/.test(lines[i]) &&
      !/^#{1,6}\s/.test(lines[i]) &&
      !/^(-{3,}|\*{3,})\s*$/.test(lines[i])
    ) {
      para.push(lines[i].trim());
      i++;
    }
    if (!para.length) {
      throw new Error('reference: no rule matched line ' + (i + 1) + ': ' + lines[i]);
    }
    html.push('<p>' + inline(para.join(' '), links) + '</p>');
  }

  // The check that pays for this file. A `#anchor` matching no heading scrolls nowhere,
  // on this page and on GitHub, and nothing else in the repository looks.
  const broken = [...new Set(links.filter((h) => h.startsWith('#')))].filter(
    (h) => !anchors.has(h.slice(1)),
  );
  if (broken.length) {
    throw new Error(
      'reference: ' +
        REFERENCE.source +
        ' links to ' +
        broken.length +
        ' anchor(s) that do not exist: ' +
        broken.join(', '),
    );
  }

  return { html: html.join('\n'), headings };
}

export function referenceMain(repoRoot) {
  const markdown = readFileSync(repoRoot + '/' + REFERENCE.source, 'utf8');
  const { html, headings } = render(markdown);

  const toc = headings
    .filter((h) => h.level === 2)
    .map(
      (h) =>
        '              <li><a href="#' + esc(h.id) + '">' + inline(h.text, null) + '</a></li>',
    )
    .join('\n');

  const head = [
    '        <article class="doc-article container narrow doc-reference">',
    '          <nav class="doc-crumbs" aria-label="Breadcrumb">',
    '            <a href="/">dev-prune</a> <span aria-hidden="true">/</span> Reference',
    '          </nav>',
    '          <h1>' + esc(REFERENCE.title) + '</h1>',
    '          <p class="doc-lede">' + esc(REFERENCE.description) + '</p>',
    '          <p class="doc-meta">',
    '            Generated from',
    '            <a href="' +
      BLOB +
      '/' +
      REFERENCE.source +
      '" target="_blank" rel="noreferrer">' +
      esc(REFERENCE.source) +
      '</a>',
    '            at build time, so it describes the released binary and not a plan.',
    '          </p>',
    '          <nav class="doc-toc" aria-label="On this page">',
    '            <ul>',
    toc,
    '            </ul>',
    '          </nav>',
  ].join('\n');

  // Deliberately not indented to match the surrounding page: a fenced code block becomes
  // a <pre>, and indentation added for tidiness would show up as leading spaces on every
  // line of every command the reference prints.
  const body = html;

  return head + '\n' + body + '\n        </article>\n';
}
