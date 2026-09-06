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
 * two columns and a balance.
 *
 * The rows are real: `devp history --pass 1` on the author's machine —
 * the scheduled pass of 2026-09-04, 1.91 GiB from four directories in
 * three repositories. Update them by running that command again, never
 * by hand.
 * ------------------------------------------------------------------ */

const ROWS = [
  {
    path: "LoginLens/node_modules",
    tool: "npm",
    eco: "js",
    mb: 1331.2,
    proof: "package-lock.json",
    verified: true,
  },
  {
    path: "LoginLens/website/node_modules",
    tool: "npm",
    eco: "js",
    mb: 284.97,
    proof: "package-lock.json",
    verified: true,
  },
  {
    path: "Vectra-180/.venv",
    tool: "uv",
    eco: "py",
    mb: 273.31,
    proof: "uv.lock",
    verified: true,
  },
  {
    path: "portfolio-creator/node_modules",
    tool: "npm",
    eco: "js",
    mb: 64.78,
    proof: "package-lock.json",
    verified: true,
  },
];

const TOTAL_MB = ROWS.filter((r) => r.verified).reduce((a, r) => a + r.mb, 0);

function fmt(mb) {
  return mb >= 1024 ? `${(mb / 1024).toFixed(2)} GiB` : `${mb.toFixed(2)} MiB`;
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
          <span className="ledger-src">devp history --pass 1</span>
        </figcaption>

        <div className="ledger-cols" aria-hidden="true">
          <span>Freed</span>
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
          <span className="ledger-total-label">Reclaimed</span>
          <Total />
          <span className="ledger-total-note">
            one scheduled pass, 2026-09-04 · devp restore --last-run puts it
            back
          </span>
        </div>
      </figure>
    </LazyMotion>
  );
}
