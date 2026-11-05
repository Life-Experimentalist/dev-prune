// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

import React from "react";
import { hydrateRoot, createRoot } from "react-dom/client";
import App from "./App.jsx";
// Self-hosted, only the weights the page sets. They used to come from Google Fonts, which
// meant every visitor's IP address went to Google to render a page that claims to be
// private. The files are split by script and fetched lazily by unicode-range, so a reader
// downloads the Latin subset and nothing else.
import "@fontsource-variable/bricolage-grotesque/opsz.css";
import "@fontsource/instrument-sans/400.css";
import "@fontsource/instrument-sans/500.css";
import "@fontsource/instrument-sans/600.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "@fontsource/jetbrains-mono/700.css";
import "./index.css";
import "./sections.css";
import "./theme.css";
import "./motion.css";
import "./ledger.css";
import "./languages.css";
import "./livenumbers.css";
import "./herofield.css";
import "./blog.css";

const root = document.getElementById("root");

// The production build ships prerendered markup, so hydrate it rather than throwing it
// away. `vite dev` serves an empty root, so fall back to a fresh render there.
if (root.hasChildNodes()) {
  hydrateRoot(
    root,
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
} else {
  createRoot(root).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}
