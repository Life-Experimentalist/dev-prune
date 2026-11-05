// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { createHash } from 'node:crypto';

// GitHub Pages cannot send response headers, so the policy rides in a <meta> tag. The
// only inline script is the pre-paint theme resolver in index.html, and it has to stay
// inline (it runs before the stylesheet to avoid a flash of the wrong theme), so it is
// allowed by hash. The hash is computed here from the built HTML rather than written
// down, so editing that script cannot leave a stale hash that silently blocks it.
//
// `connect-src` lists exactly the origins src/livenumbers.jsx reads from; a new fetch
// target has to be added here too, and the browser console says which one is blocked.
// `font-src` allows data: because Vite inlines the smallest font subsets as data URIs.
// `style-src-attr` is open because React and framer-motion set style attributes; style
// elements and stylesheets stay restricted to this origin.
function contentSecurityPolicy() {
  return {
    name: 'dev-prune-content-security-policy',
    apply: 'build',
    transformIndexHtml: {
      order: 'post',
      handler(html) {
        const scriptHashes = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map(
          ([, body]) => `'sha256-${createHash('sha256').update(body).digest('base64')}'`,
        );
        const policy = [
          "default-src 'self'",
          `script-src 'self' ${scriptHashes.join(' ')}`.trim(),
          "style-src 'self'",
          "style-src-attr 'unsafe-inline'",
          "img-src 'self' data:",
          "font-src 'self' data:",
          "connect-src 'self' https://counter.vkrishna04.me https://crates.io https://api.npmjs.org",
          "object-src 'none'",
          "base-uri 'self'",
          "form-action 'self'",
        ].join('; ');
        return [
          {
            tag: 'meta',
            attrs: { 'http-equiv': 'Content-Security-Policy', content: policy },
            injectTo: 'head-prepend',
          },
        ];
      },
    },
  };
}

// The site is served from the root of a custom domain (see public/CNAME), so absolute
// asset paths are correct — and they have to be, because the prerendered HTML is also
// what crawlers resolve `/assets/...` against.
export default defineConfig({
  plugins: [react(), contentSecurityPolicy()],
  base: '/',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
});
