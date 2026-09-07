// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

import React, { useEffect, useState } from "react";

/* ------------------------------------------------------------------ *
 * Live adoption numbers.
 *
 * docs/ROADMAP.md rejects adoption badges, with one carve-out: a number
 * may appear if the reader can look it up at the source that publishes
 * it. So every figure here is fetched client-side from the registry
 * that publishes it, labelled with exactly what it counts (downloads,
 * not "users"; the npm figure is a 30-day window, not a total), and the
 * whole row links to that source. A row whose fetch fails is simply not
 * rendered: no cached copies, no invented fallbacks. None of this is in
 * the prerendered HTML, so the page's claims never depend on it.
 *
 * The page-view counter is ViewFlare (counter.vkrishna04.me), hit once
 * per browser tab via a sessionStorage guard. It receives no query
 * parameters and no identifiers; the CLI itself has no telemetry of any
 * kind, and that stays true.
 * ------------------------------------------------------------------ */

const VIEWS_API = "https://counter.vkrishna04.me/api/views/dev-prune-site";
const VIEWS_COUNTED_KEY = "devprune-view-counted";

async function fetchJson(url, init) {
  const res = await fetch(url, init);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.json();
}

const SOURCES = [
  {
    key: "crates",
    href: "https://crates.io/crates/dev-prune",
    label: (n) =>
      n === 1
        ? "download on crates.io, all time"
        : "downloads on crates.io, all time",
    get: async () => {
      const j = await fetchJson("https://crates.io/api/v1/crates/dev-prune");
      return j.crate.downloads;
    },
  },
  {
    key: "npm",
    href: "https://www.npmjs.com/package/dev-prune",
    label: (n) =>
      n === 1
        ? "download on npm, last 30 days"
        : "downloads on npm, last 30 days",
    get: async () => {
      const j = await fetchJson(
        "https://api.npmjs.org/downloads/point/last-month/dev-prune",
      );
      return j.downloads;
    },
  },
  {
    key: "views",
    href: VIEWS_API,
    label: (n) =>
      n === 1
        ? "view of this page, all time"
        : "views of this page, all time",
    get: async () => {
      // The guard is set before the request, not after: StrictMode runs
      // effects twice in dev, and a slow response must not turn one visit
      // into two increments.
      let counted = false;
      try {
        counted = sessionStorage.getItem(VIEWS_COUNTED_KEY) === "1";
        if (!counted) sessionStorage.setItem(VIEWS_COUNTED_KEY, "1");
      } catch {
        /* private mode: read without incrementing */
        counted = true;
      }
      const j = await fetchJson(
        VIEWS_API,
        counted ? undefined : { method: "POST" },
      );
      return j.totalViews;
    },
  },
];

export default function LiveNumbers() {
  const [rows, setRows] = useState([]);

  useEffect(() => {
    let alive = true;
    Promise.allSettled(
      SOURCES.map(async (s) => ({ ...s, value: await s.get() })),
    ).then((settled) => {
      if (!alive) return;
      setRows(
        settled
          .filter(
            (r) => r.status === "fulfilled" && Number.isFinite(r.value.value),
          )
          .map((r) => r.value),
      );
    });
    return () => {
      alive = false;
    };
  }, []);

  if (rows.length === 0) return null;

  return (
    <p className="live-numbers">
      {rows.map((r) => (
        <a
          key={r.key}
          className="live-number"
          href={r.href}
          target="_blank"
          rel="noreferrer"
        >
          <span className="live-number-value">
            {r.value.toLocaleString("en-US")}
          </span>{" "}
          <span className="live-number-label">{r.label(r.value)}</span>
        </a>
      ))}
    </p>
  );
}
