// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

// The federation network: the landing page's backdrop.
//
// A slowly turning globe with a node for each federation, at its country, sized by
// the services it registers. Around each node, small points stand for its entities
// (services in ink, identity providers in green). Metadata pulses travel along
// great-circle arcs from each federation to eduGAIN, the interfederation hub, and
// fan back out to the others: that exchange is how one federation's services reach
// another's users. InCommon is drawn in its own orange.

const DEG = Math.PI / 180;

// Approximate country centers (latitude, longitude) for every country a federation
// in eduGAIN lists. Placement only needs to be roughly right.
const CENTERS = {
  AL: [41.1, 20.2], AM: [40.1, 45.0], AT: [47.5, 14.6], AU: [-25.3, 133.8], AZ: [40.1, 47.6],
  BD: [23.7, 90.4], BE: [50.5, 4.5], BF: [12.2, -1.6], BG: [42.7, 25.5], BR: [-14.2, -51.9],
  BY: [53.7, 27.9], CA: [56.1, -106.3], CH: [46.8, 8.2], CL: [-35.7, -71.5], CN: [35.9, 104.2],
  CO: [4.6, -74.3], CY: [35.1, 33.4], CZ: [49.8, 15.5], DE: [51.2, 10.4], DK: [56.3, 9.5],
  DZ: [28.0, 1.7], EC: [-1.8, -78.2], EE: [58.6, 25.0], EH: [24.2, -12.9], ES: [40.5, -3.7],
  ET: [9.1, 40.5], FI: [61.9, 25.7], FR: [46.2, 2.2], GB: [54.0, -2.5], GE: [42.3, 43.4],
  GH: [7.9, -1.0], GL: [71.7, -42.6], GR: [39.1, 21.8], HK: [22.3, 114.2], HR: [45.1, 15.2],
  HU: [47.2, 19.5], IE: [53.4, -8.2], IL: [31.0, 34.9], IN: [20.6, 79.0], IR: [32.4, 53.7],
  IS: [64.9, -19.0], IT: [41.9, 12.6], JP: [36.2, 138.3], KE: [0.0, 37.9], KG: [41.2, 74.8],
  KR: [35.9, 127.8], LK: [7.9, 80.8], LT: [55.2, 23.9], LU: [49.8, 6.1], LV: [56.9, 24.6],
  MA: [31.8, -7.1], MD: [47.4, 28.4], MK: [41.6, 21.7], ML: [17.6, -4.0], MT: [35.9, 14.4],
  MW: [-13.3, 34.3], MX: [23.6, -102.6], MY: [4.2, 102.0], MZ: [-18.7, 35.5], NG: [9.1, 8.7],
  NL: [52.1, 5.3], NO: [60.5, 8.5], NZ: [-40.9, 174.9], OM: [21.5, 55.9], PK: [30.4, 69.3],
  PL: [51.9, 19.1], PT: [39.4, -8.2], RO: [45.9, 25.0], RS: [44.0, 21.0], SA: [23.9, 45.1],
  SE: [60.1, 18.6], SG: [1.35, 103.8], SI: [46.2, 15.0], SK: [48.7, 19.7], SO: [5.2, 46.2],
  TG: [8.6, 0.8], TH: [15.9, 101.0], TJ: [38.9, 71.3], TR: [39.0, 35.2], TZ: [-6.4, 34.9],
  UA: [48.4, 31.2], UG: [1.4, 32.3], US: [39.8, -98.6], ZA: [-30.6, 22.9], ZM: [-13.1, 27.8],
};
// eduGAIN's metadata service is run by GÉANT; the hub sits over western Europe.
const HUB = { code: "eduGAIN", lat: 50.6, lon: 4.8 };

// Shown until the API answers (and if it never does).
const FALLBACK = [
  ["INCOMMON", "US", 8500, 1100], ["UK-FEDERATION", "GB", 740, 300], ["DFN-AAI", "DE", 200, 350],
  ["RENATER", "FR", 200, 250], ["IDEM", "IT", 180, 120], ["SWAMID", "SE", 120, 60],
  ["SURFCONEXT", "NL", 150, 60], ["SIR", "ES", 100, 80], ["CAF", "CA", 150, 120],
  ["AAF", "AU", 150, 50], ["CARSI", "CN", 150, 600], ["CAFE", "BR", 60, 200],
  ["GAKUNIN", "JP", 130, 300], ["SAFIRE", "ZA", 40, 30], ["RENU", "UG", 10, 20], ["TUAKIRI", "NZ", 30, 20],
].map(([code, c, services, idps]) => ({ code, name: code, countries: [c], services, idps }));

const vec = (lat, lon) => [Math.cos(lat * DEG) * Math.cos(lon * DEG), Math.cos(lat * DEG) * Math.sin(lon * DEG), Math.sin(lat * DEG)];

function slerp(a, b, t) {
  const d = Math.min(1, Math.max(-1, a[0] * b[0] + a[1] * b[1] + a[2] * b[2]));
  const w = Math.acos(d);
  if (w < 1e-6) return a.slice();
  const s = Math.sin(w);
  const ka = Math.sin((1 - t) * w) / s;
  const kb = Math.sin(t * w) / s;
  return [a[0] * ka + b[0] * kb, a[1] * ka + b[1] * kb, a[2] * ka + b[2] * kb];
}

// Deterministic jitter, so a node's entity points keep their places between frames.
function rng(seed) {
  let x = seed >>> 0 || 1;
  return () => ((x = (x * 1664525 + 1013904223) >>> 0) / 4294967296);
}
// The same color with zero alpha, for gradients that fade out without darkening.
function clear(color) {
  const m = /^#([0-9a-f]{6})$/i.exec(color);
  if (!m) return "rgba(255,255,255,0)";
  const n = parseInt(m[1], 16);
  return `rgba(${n >> 16},${(n >> 8) & 255},${n & 255},0)`;
}
const hash = (s) => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);

export function mountNetwork(canvas, { portal = null } = {}) {
  const ctx = canvas.getContext("2d");
  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const pointer = { x: 0, y: 0, active: false };
  let W = 0, H = 0, cx = 0, cy = 0, R = 0, colors = {};
  let veil = null;
  let feds = [];
  let pulses = [];
  let spin = -42; // longitude at the center of the view: the Americas and Europe both in sight
  const tilt = 22 * DEG;
  let raf = 0;
  let last = performance.now();
  let nextSpawn = 0;

  // World vector → view: x right, y up, z toward the viewer.
  function view(v, lift = 1) {
    const c = Math.cos(spin * DEG), s = Math.sin(spin * DEG);
    const x1 = v[0] * c + v[1] * s;
    const y1 = -v[0] * s + v[1] * c;
    const z1 = v[2];
    return {
      x: cx + R * lift * y1,
      y: cy - R * lift * (Math.cos(tilt) * z1 - Math.sin(tilt) * x1),
      z: Math.sin(tilt) * z1 + Math.cos(tilt) * x1,
      lift,
    };
  }
  // A lifted point behind the globe still shows once it clears the rim.
  const shows = (p) => p.z > 0 || (p.x - cx) ** 2 + (p.y - cy) ** 2 > R * R;

  function setFederations(list) {
    const hub = vec(HUB.lat, HUB.lon);
    feds = (list && list.length ? list : FALLBACK)
      .map((f) => {
        const pts = (f.countries || []).map((c) => CENTERS[c]).filter(Boolean);
        if (!pts.length) return null;
        const lat = pts.reduce((a, p) => a + p[0], 0) / pts.length;
        const lon = pts.reduce((a, p) => a + p[1], 0) / pts.length;
        const v = vec(lat, lon);
        const size = (f.services || 0) + (f.idps || 0);
        const r = rng(hash(f.code));
        // Entity points: a few per federation, more for bigger ones.
        const n = Math.min(26, Math.round(3 + 3.2 * Math.log10(size + 1) ** 1.6));
        const spread = 2.2 + 1.6 * Math.log10(size + 1);
        const idpShare = size ? (f.idps || 0) / size : 0;
        const entities = Array.from({ length: n }, () => {
          const a = r() * Math.PI * 2, d = spread * Math.sqrt(r());
          return {
            v: vec(lat + Math.sin(a) * d, lon + (Math.cos(a) * d) / Math.max(0.3, Math.cos(lat * DEG))),
            idp: r() < idpShare,
            phase: r() * Math.PI * 2,
            speed: 0.6 + r() * 1.4,
          };
        });
        // The arc to the hub, lifted in proportion to its length.
        const w = Math.acos(Math.min(1, v[0] * hub[0] + v[1] * hub[1] + v[2] * hub[2]));
        const arc = Array.from({ length: 33 }, (_, i) => {
          const t = i / 32;
          return { v: slerp(v, hub, t), lift: 1 + Math.sin(Math.PI * t) * (0.03 + 0.2 * (w / Math.PI)) };
        });
        return { ...f, lat, lon, v, size, entities, arc, home: f.code === "INCOMMON", flash: 0 };
      })
      .filter(Boolean)
      .sort((a, b) => a.size - b.size);
    pulses = [];
    if (reduceMotion) draw(0);
  }

  function resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const r = canvas.getBoundingClientRect();
    W = r.width;
    H = r.height;
    canvas.width = Math.round(W * dpr);
    canvas.height = Math.round(H * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    // Matches the layout in styles.css: beside the search when wide, below it when not.
    if (W >= 900) {
      R = Math.min(W * 0.24, H * 0.42);
      cx = W - Math.max(R * 1.25, W * 0.27);
      cy = H * 0.5;
    } else {
      R = Math.min(W * 0.46, 300);
      const top = portal ? portal.getBoundingClientRect().bottom - canvas.getBoundingClientRect().top : H * 0.5;
      cx = W / 2;
      cy = Math.max(top + R * 0.9, H - R * 1.05);
    }
    const cs = getComputedStyle(canvas);
    const v = (n, d) => cs.getPropertyValue(n).trim() || d;
    colors = {
      ink: v("--ink", "#00365e"),
      brand: v("--brand", "#ff5a00"),
      second: v("--second", "#16750e"),
      line: v("--line", "#e6e3d2"),
      bg: v("--bg", "#f7f5eb"),
      surface: v("--surface", "#ffffff"),
      muted: v("--muted", "#4e6478"),
    };
    // Keep the search box legible: a soft wash of the page color behind it.
    veil = null;
    if (portal) {
      const c = canvas.getBoundingClientRect();
      const p = portal.getBoundingClientRect();
      veil = { x: p.left - c.left + p.width / 2, y: p.top - c.top + p.height / 2, r: Math.max(p.width, p.height) * 0.6 };
    }
    if (reduceMotion) draw(0);
  }

  function pick() {
    // Bigger federations publish more, so they send more pulses.
    const total = feds.reduce((a, f) => a + Math.log10(f.size + 10), 0);
    let x = Math.random() * total;
    for (const f of feds) if ((x -= Math.log10(f.size + 10)) <= 0) return f;
    return feds[feds.length - 1];
  }

  function step(dt, now) {
    spin += dt * 4.5;
    if (now > nextSpawn && feds.length && pulses.length < 48) {
      pulses.push({ fed: pick(), out: false, t: 0, speed: 0.45 + Math.random() * 0.25 });
      nextSpawn = now + 140 + Math.random() * 220;
    }
    for (const p of pulses) {
      p.t += dt * p.speed;
      if (p.t >= 1 && !p.done) {
        p.done = true;
        if (p.out) p.fed.flash = 1;
        else for (let i = 0; i < 2; i++) pulses.push({ fed: pick(), out: true, t: 0, speed: 0.45 + Math.random() * 0.25 });
      }
    }
    pulses = pulses.filter((p) => !p.done);
    for (const f of feds) f.flash = Math.max(0, f.flash - dt * 1.6);
  }

  // A point along a federation's arc: t = 0 at the federation, 1 at the hub.
  function along(f, t) {
    const x = t * (f.arc.length - 1);
    const i = Math.min(f.arc.length - 2, Math.floor(x));
    const k = x - i;
    const a = f.arc[i], b = f.arc[i + 1];
    const v = [a.v[0] + (b.v[0] - a.v[0]) * k, a.v[1] + (b.v[1] - a.v[1]) * k, a.v[2] + (b.v[2] - a.v[2]) * k];
    const n = Math.hypot(...v);
    return view([v[0] / n, v[1] / n, v[2] / n], a.lift + (b.lift - a.lift) * k);
  }

  function draw(time) {
    ctx.clearRect(0, 0, W, H);
    if (!R) return;

    // The globe: a lit disk, its rim, and a faint graticule on the near side.
    const g = ctx.createRadialGradient(cx - R * 0.35, cy - R * 0.4, R * 0.1, cx, cy, R);
    g.addColorStop(0, colors.surface);
    g.addColorStop(1, colors.bg);
    ctx.globalAlpha = 0.9;
    ctx.fillStyle = g;
    ctx.beginPath();
    ctx.arc(cx, cy, R, 0, Math.PI * 2);
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.strokeStyle = colors.line;
    ctx.lineWidth = 1.5;
    ctx.stroke();

    ctx.lineWidth = 1;
    ctx.strokeStyle = colors.ink;
    ctx.globalAlpha = 0.08;
    const line = (pts) => {
      ctx.beginPath();
      let pen = false;
      for (const v of pts) {
        const p = view(v);
        if (p.z > 0) pen ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y);
        pen = p.z > 0;
      }
      ctx.stroke();
    };
    for (let lat = -60; lat <= 60; lat += 30) line(Array.from({ length: 73 }, (_, i) => vec(lat, i * 5)));
    for (let lon = 0; lon < 180; lon += 30) line(Array.from({ length: 73 }, (_, i) => vec(i * 5 - 180, lon)));
    ctx.globalAlpha = 1;

    // Arcs to the hub.
    for (const f of feds) {
      ctx.beginPath();
      let pen = false;
      for (let i = 0; i < f.arc.length; i++) {
        const p = view(f.arc[i].v, f.arc[i].lift);
        const on = shows(p);
        if (on) pen ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y);
        pen = on;
      }
      ctx.strokeStyle = f.home ? colors.brand : colors.ink;
      ctx.globalAlpha = f.home ? 0.55 : 0.14 + 0.1 * Math.min(1, Math.log10(f.size + 1) / 3);
      ctx.lineWidth = f.home ? 1.6 : 1;
      ctx.stroke();
    }

    // Entities, twinkling.
    for (const f of feds) {
      for (const e of f.entities) {
        const p = view(e.v);
        if (p.z <= 0) continue;
        ctx.globalAlpha = (0.25 + 0.45 * (0.5 + 0.5 * Math.sin(time * 0.001 * e.speed + e.phase))) * Math.min(1, p.z * 2.5);
        ctx.fillStyle = e.idp ? colors.second : colors.ink;
        ctx.fillRect(p.x - 1, p.y - 1, 2, 2);
      }
    }

    // Federation nodes, with a ring when metadata arrives.
    const labels = [];
    for (const f of feds) {
      const p = view(f.v);
      if (p.z <= 0) continue;
      const r = (f.home ? 3.5 : 1.6) + 1.5 * Math.log10(f.size + 1);
      const fade = Math.min(1, p.z * 3);
      ctx.globalAlpha = fade * (f.home ? 1 : 0.8);
      ctx.fillStyle = f.home ? colors.brand : colors.ink;
      ctx.beginPath();
      ctx.arc(p.x, p.y, r, 0, Math.PI * 2);
      ctx.fill();
      if (f.flash > 0) {
        ctx.globalAlpha = fade * f.flash;
        ctx.strokeStyle = colors.brand;
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.arc(p.x, p.y, r + 6 * (1 - f.flash) + 2, 0, Math.PI * 2);
        ctx.stroke();
      }
      labels.push({ f, p, r, fade });
    }

    // The hub.
    const hp = view(vec(HUB.lat, HUB.lon));
    if (hp.z > 0) {
      ctx.globalAlpha = Math.min(1, hp.z * 3);
      ctx.strokeStyle = colors.second;
      ctx.lineWidth = 2;
      for (const rr of [5, 9 + 2 * Math.sin(time * 0.003)]) {
        ctx.beginPath();
        ctx.arc(hp.x, hp.y, rr, 0, Math.PI * 2);
        ctx.stroke();
      }
    }

    // Metadata pulses, each with a short tail.
    for (const pl of pulses) {
      for (let k = 0; k < 5; k++) {
        const t = pl.out ? 1 - (pl.t - k * 0.025) : pl.t - k * 0.025;
        if (t < 0 || t > 1) continue;
        const p = along(pl.fed, t);
        if (!shows(p)) continue;
        ctx.globalAlpha = (1 - k / 5) * (p.z > 0 ? 1 : 0.5);
        ctx.fillStyle = colors.brand;
        ctx.beginPath();
        ctx.arc(p.x, p.y, 2.4 - k * 0.35, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    // The search box's wash.
    if (veil) {
      const v = ctx.createRadialGradient(veil.x, veil.y, veil.r * 0.35, veil.x, veil.y, veil.r);
      v.addColorStop(0, colors.bg);
      v.addColorStop(1, clear(colors.bg)); // fading to transparent black would gray the page
      ctx.globalAlpha = 0.88;
      ctx.fillStyle = v;
      ctx.fillRect(veil.x - veil.r, veil.y - veil.r, veil.r * 2, veil.r * 2);
    }

    // Labels: the largest federations in view, plus whichever one the pointer is on.
    ctx.font = `600 ${W < 640 ? 10 : 11}px Poppins, system-ui, sans-serif`;
    ctx.textBaseline = "middle";
    let hover = null;
    if (pointer.active) {
      let best = 18 * 18;
      for (const l of labels) {
        const d = (l.p.x - pointer.x) ** 2 + (l.p.y - pointer.y) ** 2;
        if (d < best) (best = d), (hover = l);
      }
    }
    const shown = labels.filter((l) => l.p.z > 0.25).sort((a, b) => b.f.size - a.f.size).slice(0, W < 640 ? 5 : 9);
    if (hover && !shown.includes(hover)) shown.push(hover);
    if (hp.z > 0.25) shown.unshift({ f: { code: HUB.code, size: 0 }, p: hp, r: 9, fade: 1, hub: true });
    const taken = [];
    for (const l of shown) {
      const text = l === hover ? `${l.f.name || l.f.code} · ${(l.f.services || 0).toLocaleString()} services` : l.f.code;
      const w = ctx.measureText(text).width;
      const x = l.p.x + l.r + 5, y = l.p.y;
      if (veil && Math.hypot(x + w / 2 - veil.x, y - veil.y) < veil.r * 0.75 && l !== hover) continue;
      if (x + w > W - 4) continue; // never cut a label off at the edge
      if (l !== hover && taken.some((b) => x < b.x + b.w + 8 && x + w + 8 > b.x && Math.abs(y - b.y) < 15)) continue;
      taken.push({ x, y, w });
      ctx.globalAlpha = l === hover ? 1 : 0.6 + 0.4 * l.fade;
      // A halo in the page color keeps labels legible over dense nodes and arcs.
      ctx.lineJoin = "round";
      ctx.lineWidth = 3.5;
      ctx.strokeStyle = colors.bg;
      ctx.strokeText(text, x, y);
      ctx.fillStyle = l.hub ? colors.second : l.f.home || l === hover ? colors.brand : colors.ink;
      ctx.fillText(text, x, y);
    }
    ctx.globalAlpha = 1;
  }

  function frame(now) {
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;
    step(dt, now);
    draw(now);
    raf = requestAnimationFrame(frame);
  }

  const onMove = (e) => {
    const r = canvas.getBoundingClientRect();
    pointer.x = e.clientX - r.left;
    pointer.y = e.clientY - r.top;
    pointer.active = true;
    if (reduceMotion) draw(0);
  };
  const onLeave = () => (pointer.active = false);
  const ro = new ResizeObserver(resize);
  ro.observe(canvas);
  canvas.addEventListener("pointermove", onMove, { passive: true });
  canvas.addEventListener("pointerleave", onLeave);
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  mq.addEventListener("change", resize);

  resize();
  setFederations(null);
  if (!reduceMotion) raf = requestAnimationFrame(frame);

  return {
    setFederations,
    refreshColors: resize,
    stop() {
      cancelAnimationFrame(raf);
      ro.disconnect();
      canvas.removeEventListener("pointermove", onMove);
      canvas.removeEventListener("pointerleave", onLeave);
      mq.removeEventListener("change", resize);
    },
  };
}
