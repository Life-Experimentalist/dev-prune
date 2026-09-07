// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

import React, { useEffect, useRef } from "react";

/* ------------------------------------------------------------------ *
 * The reclaim field: the hero's ambient layer.
 *
 * Not a starfield. The subject is a disk, so the field is an allocation
 * map: small square blocks drifting very slowly, most of them barely
 * there. Every few seconds a soft vertical sweep crosses the hero, and
 * the blocks it touches flare in the brand emerald and lift away. Green
 * already means exactly one thing on this page (this directory was
 * proven rebuildable), so the one orchestrated moment is the product's
 * own semantics: a verification pass releasing space.
 *
 * Ground rules the implementation must keep:
 *   - decoration never gets the gold (--reclaimed is reserved for data);
 *   - alphas stay low enough that text contrast is untouched;
 *   - prefers-reduced-motion gets a single static frame, no loop;
 *   - the loop pauses when the tab is hidden or the hero is offscreen;
 *   - DPR is capped at 2, and everything is plain canvas 2D.
 * ------------------------------------------------------------------ */

const MAX_DPR = 2;
const REST_MS = 4500;
const TRAVEL_MS = 6500;
const CYCLE_MS = REST_MS + TRAVEL_MS;
const BAND_PX = 110;
const GLOW_PX = 30;

const PALETTES = {
  dark: {
    block: "rgb(154, 163, 189)",
    verified: "rgb(16, 185, 129)",
    glow: "16, 185, 129",
    blockAlpha: 0.16,
    flareAlpha: 0.55,
    bandAlpha: 0.045,
  },
  light: {
    block: "rgb(84, 92, 120)",
    verified: "rgb(4, 120, 87)",
    glow: "4, 120, 87",
    blockAlpha: 0.13,
    flareAlpha: 0.4,
    bandAlpha: 0.035,
  },
};

function makeGlowSprite(rgb) {
  const c = document.createElement("canvas");
  c.width = GLOW_PX * 2;
  c.height = GLOW_PX * 2;
  const g = c.getContext("2d");
  const grad = g.createRadialGradient(
    GLOW_PX,
    GLOW_PX,
    0,
    GLOW_PX,
    GLOW_PX,
    GLOW_PX,
  );
  grad.addColorStop(0, `rgba(${rgb}, 0.85)`);
  grad.addColorStop(0.4, `rgba(${rgb}, 0.25)`);
  grad.addColorStop(1, `rgba(${rgb}, 0)`);
  g.fillStyle = grad;
  g.fillRect(0, 0, GLOW_PX * 2, GLOW_PX * 2);
  return c;
}

export default function HeroField() {
  const canvasRef = useRef(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return undefined;
    const ctx = canvas.getContext("2d");
    if (!ctx) return undefined;

    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)");
    const themeOf = () =>
      document.documentElement.getAttribute("data-theme") === "light"
        ? "light"
        : "dark";

    let pal = PALETTES[themeOf()];
    let sprite = null;
    let particles = [];
    let w = 0;
    let h = 0;
    let raf = 0;
    let idleHandle = 0;
    let running = false;
    let pageVisible = !document.hidden;
    let onScreen = true;
    let t0 = 0;
    let last = 0;

    const seed = () => {
      const count = Math.max(60, Math.min(220, Math.round((w * h) / 9000)));
      particles = Array.from({ length: count }, () => ({
        x: Math.random() * w,
        y: Math.random() * h,
        size: 1 + Math.random() * 1.6,
        base: 0.5 + Math.random() * 0.5,
        tw: 0.4 + Math.random() * 1.2,
        ph: Math.random() * Math.PI * 2,
        vy: -(2 + Math.random() * 5),
        drift: (Math.random() - 0.5) * 3,
        flare: 0,
      }));
    };

    const drawStatic = () => {
      ctx.clearRect(0, 0, w, h);
      ctx.fillStyle = pal.block;
      for (const p of particles) {
        ctx.globalAlpha = p.base * pal.blockAlpha;
        ctx.fillRect(p.x, p.y, p.size, p.size);
      }
      // A handful of already-verified blocks, so the still frame carries
      // the same idea the animation does.
      ctx.fillStyle = pal.verified;
      for (let i = 0; i < particles.length; i += 23) {
        const p = particles[i];
        ctx.globalAlpha = pal.flareAlpha * 0.5;
        ctx.fillRect(p.x, p.y, p.size, p.size);
      }
      ctx.globalAlpha = 1;
    };

    const draw = (ts) => {
      ctx.clearRect(0, 0, w, h);

      const tc = (ts - t0) % CYCLE_MS;
      let sweepX = -1;
      if (tc >= REST_MS) {
        const phase = (tc - REST_MS) / TRAVEL_MS;
        sweepX = phase * (w + BAND_PX * 2) - BAND_PX;
        const grad = ctx.createLinearGradient(
          sweepX - BAND_PX,
          0,
          sweepX + BAND_PX,
          0,
        );
        grad.addColorStop(0, `rgba(${pal.glow}, 0)`);
        grad.addColorStop(0.5, `rgba(${pal.glow}, ${pal.bandAlpha})`);
        grad.addColorStop(1, `rgba(${pal.glow}, 0)`);
        ctx.fillStyle = grad;
        ctx.fillRect(sweepX - BAND_PX, 0, BAND_PX * 2, h);
      }

      const tSec = ts / 1000;
      for (const p of particles) {
        if (sweepX >= 0) {
          const d = Math.abs(p.x - sweepX);
          if (d < BAND_PX) p.flare = Math.max(p.flare, 1 - d / BAND_PX);
        }
        const twinkle = 0.75 + 0.25 * Math.sin(tSec * p.tw + p.ph);
        const a = p.base * pal.blockAlpha * twinkle;
        ctx.fillStyle = pal.block;
        ctx.globalAlpha = a;
        ctx.fillRect(p.x, p.y, p.size, p.size);
        if (p.flare > 0.02) {
          ctx.globalAlpha = p.flare * pal.flareAlpha * 0.7;
          ctx.drawImage(
            sprite,
            p.x + p.size / 2 - GLOW_PX,
            p.y + p.size / 2 - GLOW_PX,
            GLOW_PX * 2,
            GLOW_PX * 2,
          );
          ctx.fillStyle = pal.verified;
          ctx.globalAlpha = Math.min(1, a + p.flare * pal.flareAlpha);
          ctx.fillRect(p.x, p.y, p.size, p.size);
        }
      }
      ctx.globalAlpha = 1;
    };

    const step = (dt) => {
      for (const p of particles) {
        p.y += p.vy * dt * (1 + p.flare * 3);
        p.x += p.drift * dt;
        p.flare *= Math.exp(-1.8 * dt);
        if (p.y < -6) {
          p.y = h + 6;
          p.x = Math.random() * w;
          p.flare = 0;
        }
        if (p.x < -6) p.x = w + 6;
        else if (p.x > w + 6) p.x = -6;
      }
    };

    const frame = (ts) => {
      raf = 0;
      if (!pageVisible || !onScreen || reduced.matches) {
        running = false;
        return;
      }
      if (!t0) t0 = ts;
      const dt = Math.min(0.05, (ts - last) / 1000 || 0.016);
      last = ts;
      step(dt);
      draw(ts);
      raf = requestAnimationFrame(frame);
    };

    const start = () => {
      if (reduced.matches) {
        drawStatic();
        return;
      }
      if (running || !pageVisible || !onScreen) return;
      running = true;
      last = performance.now();
      raf = requestAnimationFrame(frame);
    };

    const stop = () => {
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
      running = false;
    };

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      w = rect.width;
      h = rect.height;
      const dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      seed();
      if (reduced.matches) drawStatic();
    };

    const onVisibility = () => {
      pageVisible = !document.hidden;
      if (pageVisible) start();
      else stop();
    };

    const onMotionPref = () => {
      if (reduced.matches) {
        stop();
        drawStatic();
      } else {
        start();
      }
    };

    const io = new IntersectionObserver(([entry]) => {
      onScreen = entry.isIntersecting;
      if (onScreen) start();
      else stop();
    });

    const ro = new ResizeObserver(() => resize());

    const mo = new MutationObserver(() => {
      pal = PALETTES[themeOf()];
      sprite = makeGlowSprite(pal.glow);
      if (reduced.matches) drawStatic();
    });

    // Everything waits for an idle moment so hydration and the first
    // paint are never competing with the field.
    const begin = () => {
      sprite = makeGlowSprite(pal.glow);
      resize();
      io.observe(canvas);
      ro.observe(canvas);
      mo.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ["data-theme"],
      });
      document.addEventListener("visibilitychange", onVisibility);
      if (reduced.addEventListener)
        reduced.addEventListener("change", onMotionPref);
      start();
    };

    if (typeof window.requestIdleCallback === "function") {
      idleHandle = window.requestIdleCallback(begin, { timeout: 800 });
    } else {
      idleHandle = window.setTimeout(begin, 200);
    }

    return () => {
      stop();
      if (typeof window.cancelIdleCallback === "function")
        window.cancelIdleCallback(idleHandle);
      else window.clearTimeout(idleHandle);
      io.disconnect();
      ro.disconnect();
      mo.disconnect();
      document.removeEventListener("visibilitychange", onVisibility);
      if (reduced.removeEventListener)
        reduced.removeEventListener("change", onMotionPref);
    };
  }, []);

  return <canvas ref={canvasRef} className="hero-field" aria-hidden="true" />;
}
