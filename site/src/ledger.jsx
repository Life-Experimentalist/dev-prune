// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

import React, { useEffect, useRef, useState } from "react";
import {
  animate,
  domAnimation,
  LazyMotion,
  m,
  useMotionValue,
  useReducedMotion,
} from "framer-motion";
import { Check, X } from "lucide-react";

/* ------------------------------------------------------------------ *
 * The Reclaim Ledger.
 *
 * Every other tool in this category opens with a number and the word
 * "GB". The number is not the argument — anything can delete a folder and
 * report a total. The argument is the second column: for each directory,
 * the artefact that proves it can be rebuilt. So the hero is a receipt,
 * two columns and a balance, and the row that is refused is on it too,
 * because a ledger that only records the wins is not a ledger.
 *
 * The figures are one `devp run --dry-run` pass over the same tree the
 * terminal below it is showing, and they add up.
 * ------------------------------------------------------------------ */

const ROWS = [
  {
    path: "frontend/node_modules",
    tool: "pnpm",
    eco: "js",
    mb: 412.7,
    proof: "pnpm-lock.yaml",
    verified: true,
  },
  {
    path: "services/api/.venv",
    tool: "uv",
    eco: "py",
    mb: 188.2,
    proof: "uv.lock",
    verified: true,
  },
  {
    path: "tools/cli/target",
    tool: "cargo",
    eco: "rust",
    mb: 1447.1,
    proof: "Cargo.lock",
    verified: true,
  },
  {
    path: "vendor/legacy-php",
    tool: "composer",
    eco: "php",
    mb: 96.4,
    proof: "no composer.lock",
    verified: false,
  },
];

const TOTAL_MB = ROWS.filter((r) => r.verified).reduce((a, r) => a + r.mb, 0);

function fmt(mb) {
  return mb >= 1024 ? `${(mb / 1024).toFixed(2)} GB` : `${mb.toFixed(1)} MB`;
}

/* The bar is a share of the largest row, not of the total: at these sizes a
   share-of-total bar would render the two smallest rows as slivers, which is
   the opposite of what a disk analyser is for. */
const WIDEST = Math.max(...ROWS.map((r) => r.mb));

function Total() {
  // Seeded with the final value so the prerendered HTML — what a crawler and
  // a reader without JavaScript get — already shows the answer. The count-up
  // resets it and replays only once React is running.
  const mv = useMotionValue(TOTAL_MB);
  const [text, setText] = useState(fmt(TOTAL_MB));
  const reduce = useReducedMotion();

  useEffect(() => {
    if (reduce) return;
    mv.set(0);
    const stop = mv.on("change", (v) => setText(fmt(v)));
    const controls = animate(mv, TOTAL_MB, {
      duration: 1.1,
      delay: 0.75,
      ease: [0.16, 1, 0.3, 1],
    });
    return () => {
      controls.stop();
      stop();
    };
  }, [mv, reduce]);

  return <span className="ledger-total-value">{text}</span>;
}

export default function ReclaimLedger() {
  const reduce = useReducedMotion();
  const [armed, setArmed] = useState(false);
  const ref = useRef(null);

  useEffect(() => setArmed(true), []);

  const rowMotion = (i) =>
    !armed || reduce
      ? {}
      : {
          initial: { opacity: 0, y: 14 },
          animate: { opacity: 1, y: 0 },
          transition: {
            duration: 0.45,
            delay: 0.12 + i * 0.11,
            ease: [0.16, 1, 0.3, 1],
          },
        };

  return (
    /* `LazyMotion` + `m` instead of the full `motion` component: the ledger
       only needs DOM animation, and loading the gesture and layout engines
       for it would put ~25 kB on a landing page's critical path. */
    <LazyMotion features={domAnimation} strict>
      <figure className="ledger" ref={ref}>
        <figcaption className="ledger-head">
          <span className="ledger-title">The reclaim ledger</span>
          <span className="ledger-src">devp run --dry-run</span>
        </figcaption>

        <div className="ledger-cols" aria-hidden="true">
          <span>On disk</span>
          <span>Rebuilt by</span>
        </div>

        <ol className="ledger-rows">
          {ROWS.map((r, i) => (
            <m.li
              key={r.path}
              className={r.verified ? "ledger-row" : "ledger-row is-refused"}
              style={{ "--eco": `var(--eco-${r.eco})` }}
              {...rowMotion(i)}
            >
              <div className="ledger-disk">
                <code className="ledger-path">{r.path}</code>
                <span className="ledger-size">{fmt(r.mb)}</span>
                <span
                  className="ledger-bar"
                  style={{ width: `${(r.mb / WIDEST) * 100}%` }}
                  aria-hidden="true"
                />
              </div>

              <div className="ledger-mark" aria-hidden="true">
                {r.verified ? <Check size={13} /> : <X size={13} />}
              </div>

              <div className="ledger-proof">
                <code>{r.proof}</code>
                <span className="ledger-verdict">
                  {r.verified
                    ? `${r.tool} can rebuild it`
                    : "kept — nothing can prove it"}
                </span>
              </div>
            </m.li>
          ))}
        </ol>

        <div className="ledger-total">
          <span className="ledger-total-label">Reclaimable</span>
          <Total />
          <span className="ledger-total-note">
            3 of 4 directories · nothing deleted
          </span>
        </div>
      </figure>
    </LazyMotion>
  );
}
