// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

// InCatalog front end: a static, build-free single-page app over the catalog API.
//
// Every string shown here comes from federation metadata that thousands of
// registrants wrote, so nothing is ever inserted as HTML: all rendering goes through
// h(), which only sets text and vetted attributes, and every URL passes safeUrl().

import { mountNetwork } from "./network.js";

const API = (window.CATALOG_CONFIG?.apiBase || "").replace(/\/$/, "");
const main = document.getElementById("main");
let globe = null;
let metaCache = null;

// ---------- utilities ----------

function h(tag, attrs, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === "class") el.className = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (k === "href" || k === "src") {
      const u = k === "src" ? safeImg(v) : v.startsWith("#") ? v : safeUrl(v);
      if (u) el.setAttribute(k, u);
    } else el.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of children.flat(Infinity)) {
    if (c == null || c === false) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

function safeUrl(u) {
  try {
    const url = new URL(u, location.href);
    return ["http:", "https:", "mailto:"].includes(url.protocol) ? url.href : null;
  } catch {
    return null;
  }
}

function safeImg(u) {
  if (typeof u !== "string") return null;
  if (/^data:image\/(png|jpe?g|gif|webp|svg\+xml)[;,]/i.test(u)) return u;
  return u.startsWith("https://") ? u : null;
}

const fmt = (n) => (n ?? 0).toLocaleString("en-US");
const flag = (cc) =>
  cc && cc.length === 2 ? String.fromCodePoint(...[...cc.toUpperCase()].map((c) => 0x1f1a5 + c.charCodeAt(0))) : "";

async function api(path, params) {
  const qs = params ? "?" + new URLSearchParams(Object.entries(params).filter(([, v]) => v != null && v !== "")) : "";
  const r = await fetch(`${API}${path}${qs}`, { headers: { Accept: "application/json" } });
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
  return r.json();
}

async function meta() {
  return (metaCache ??= api("/v1/meta"));
}

// Example searches (/v1/examples), each tested to read as the filters it shows.
let examplesCache = null;
async function examples() {
  examplesCache ??= api("/v1/examples").catch(() => ((examplesCache = null), []));
  return examplesCache;
}

// Federation code or registration authority (lowercase) → { name, ra }, for labeling
// filters and for telling that a code in a link and a registrar in a facet are one federation.
let fedIndexCache = null;
async function fedIndex() {
  fedIndexCache ??= api("/v1/federations")
    .then((feds) =>
      new Map(feds.flatMap((f) => [f.code, f.registration_authority].filter(Boolean).map((k) => [k.toLowerCase(), { name: f.name, ra: f.registration_authority }]))),
    )
    .catch(() => ((fedIndexCache = null), new Map()));
  return fedIndexCache;
}

// The same comparison the API makes: by registrar, ignoring case and a trailing slash.
const fedKey = (index, v) => (index?.get(v.toLowerCase())?.ra ?? v).trim().replace(/\/+$/, "").toLowerCase();

// Filters that take several values, kept comma-separated in the URL as the API reads them:
// a service must carry every category, and may be registered by any of the federations,
// in any of the countries or regions, or be in any of the environments.
const MULTI = { category: "all", federation: "any", country: "any", region: "any", environment: "any", sharing: "any" };
// Region names read "the European Union" in a sentence and "European Union" in a list.
const bare = (label) => label.replace(/^the /, "");
const listOf = (v) => (v || "").split(",").map((x) => x.trim()).filter(Boolean);

function parseHash() {
  const raw = location.hash.replace(/^#/, "") || "/";
  const [path, query] = raw.split("?");
  // A hand-written link may repeat a multi-value key (federation=A&federation=B).
  const params = {};
  for (const [k, v] of new URLSearchParams(query || "")) params[k] = k in MULTI && params[k] ? `${params[k]},${v}` : v;
  return { parts: path.split("/").filter(Boolean), params };
}

function go(path, params) {
  const qs = params ? new URLSearchParams(Object.entries(params).filter(([, v]) => v != null && v !== "")).toString() : "";
  location.hash = `#${path}${qs ? "?" + qs : ""}`;
}

const CATEGORY_LABELS = {
  "research-and-scholarship": "R&S",
  anonymous: "Anonymous",
  pseudonymous: "Pseudonymous",
  personalized: "Personalized",
  "code-of-conduct-v2": "CoCo v2",
  "code-of-conduct-v1": "CoCo v1",
  "hide-from-discovery": "Hidden from discovery",
  sirtfi: "Sirtfi",
  sirtfi2: "Sirtfi v2",
  "registered-by-incommon": "InCommon",
};
const AVAILABILITY = {
  "incommon-only": "InCommon only",
  "incommon-and-edugain": "InCommon + eduGAIN",
  "edugain-imported-by-incommon": "eduGAIN, in InCommon",
  "edugain-only": "eduGAIN only",
  "incommon-aggregate-only": "InCommon aggregate only",
};
const AVAILABILITY_HELP = {
  "incommon-only": "Registered by InCommon and not exported to eduGAIN, so it is reachable only from InCommon identity providers.",
  "incommon-and-edugain": "Registered by InCommon and published to eduGAIN, so identity providers in other federations can see it too.",
  "edugain-imported-by-incommon": "Registered by another federation and included in InCommon's eduGAIN import, so InCommon identity providers see it.",
  "edugain-only": "Registered by another federation and in eduGAIN, but not in InCommon's (filtered) import.",
  "incommon-aggregate-only": "Registered by another federation and present in InCommon's aggregate, but not in eduGAIN's.",
};
// Sirtfi is near-universal and the InCommon tag is implied by the registrar, so
// cards leave them off to keep the meaningful tags visible.
const ENVIRONMENT = { production: "Production", test: "Test", staging: "Staging", development: "Development", unknown: "Unknown" };
const NON_PRODUCTION = { test: "test", staging: "staging", development: "dev" };
const SHARING = { multilateral: "Open to any institution", bilateral: "For one institution", unknown: "Unknown" };
const SHARING_BADGE = { multilateral: "any institution", bilateral: "one institution" };
// What "open to any institution" does and does not promise, wherever it is shown.
const SHARING_CAVEAT =
  "Open to users of any institution's identity provider in principle. Whether you get in still depends on your identity provider releasing the attributes the service needs, and on any subscription it requires.";
const SHARING_SOURCE = {
  metadata: "Decided from the metadata",
  generated: "Claude's reading of this service's metadata",
};
// What decided a service's environment (`environment_source` on results).
const ENVIRONMENT_SOURCE = {
  metadata: "Read from a marker in the entityID, the name, or the endpoint hosts",
  generated: "Claude's reading of this service's metadata",
};
const BASIS_NOTES = {
  metadata: "Based only on this service's metadata.",
  metadata_and_general_knowledge: "Based on this service's metadata plus general knowledge about the product it names.",
};
const QUIET_CATEGORIES = new Set(["sirtfi", "registered-by-incommon"]);

function monogram(name) {
  return (name || "?").replace(/^(the|university of)\s+/i, "").trim().charAt(0).toUpperCase() || "?";
}

// The monogram shows until the image has loaded, and stays if it never does. The
// image has to be in the page from the start: a lazy image outside the page never loads.
function logoEl(url, name, large, note) {
  const box = h("div", { class: `logo${large ? " lg" : ""}`, "aria-hidden": "true", title: note }, h("span", {}, monogram(name)));
  if (safeImg(url)) {
    const img = h("img", { src: url, alt: "", loading: "lazy", decoding: "async", referrerpolicy: "no-referrer" });
    img.addEventListener("load", () => box.classList.add("loaded"));
    img.addEventListener("error", () => img.remove());
    box.append(img);
  }
  return box;
}

function serviceCard(s) {
  const cats = s.categories.filter((c) => !QUIET_CATEGORIES.has(c));
  return h(
    "a",
    { class: "card", href: `#/services/${s.id}` },
    h("div", { class: "card-top" }, logoEl(s.logo, s.name, false, s.logo_source === "website" ? "Logo from the organization's website" : null), h("div", { style: "min-width:0" }, h("h2", {}, s.name), s.organization && h("div", { class: "org" }, s.organization))),
    s.description &&
      h(
        "p",
        { title: s.description_source === "generated" ? "Summary written by Claude from this service's metadata" : null },
        s.description_source === "generated" && h("span", { class: "gen-mark", "aria-label": "Generated summary:" }, "✦ "),
        s.description,
      ),
    h(
      "div",
      { class: "badges" },
      NON_PRODUCTION[s.environment] && h("span", { class: "badge env", title: ENVIRONMENT_SOURCE[s.environment_source] }, NON_PRODUCTION[s.environment]),
      SHARING_BADGE[s.sharing] &&
        h(
          "span",
          {
            class: `badge sharing ${s.sharing}`,
            title: [SHARING_SOURCE[s.sharing_source], s.sharing === "multilateral" ? SHARING_CAVEAT : null].filter(Boolean).join(". "),
          },
          s.sharing_source === "generated" && h("span", { class: "gen-mark", "aria-label": "Generated:" }, "✦ "),
          SHARING_BADGE[s.sharing],
        ),
      s.availability && h("span", { class: "badge avail" }, AVAILABILITY[s.availability]),
      cats.map((c) => h("span", { class: "badge cat" }, CATEGORY_LABELS[c] || c)),
      s.federation_name && h("span", { class: "badge" }, `${flag(s.countries[0])} ${s.federation_name}`.trim()),
      s.curated?.provider_type && h("span", { class: "badge cur" }, s.curated.provider_type),
    ),
  );
}

function setNav(section) {
  document.querySelectorAll("[data-nav]").forEach((a) => a.toggleAttribute("aria-current", a.dataset.nav === section));
  document.querySelectorAll("[data-nav][aria-current]").forEach((a) => a.setAttribute("aria-current", "page"));
}

// replaceChildren() would print `false` from a `cond && node` child, so drop those.
function fill(el, ...nodes) {
  el.replaceChildren(...nodes.flat(Infinity).filter((n) => n != null && n !== false && n !== ""));
}

function render(...nodes) {
  fill(main, ...nodes);
}

const loading = () => h("div", { class: "loading" }, "descending…");
const failure = (e) =>
  h("div", { class: "error" }, `The catalog API did not answer (${e.message}). Check that it is running and that config.js points at it.`);

// ---------- natural-language search ----------

const answers = new Map();

// /v1/ask reads the query and finds results for it, loosening the reading when
// it matches nothing. Cached per page load so the results view reuses it.
async function answer(text) {
  if (!answers.has(text)) answers.set(text, api("/v1/ask", { q: text }));
  return answers.get(text);
}

const clean = (o) => Object.fromEntries(Object.entries(o).filter(([, v]) => v != null && v !== ""));

// Every search goes through /v1/ask. A query that names no filters and needs no
// loosening becomes a plain search, so typing a service's name behaves as before.
async function interpretAndGo(text, form) {
  text = text.trim();
  if (!text) return go("/search");
  form?.classList.add("busy");
  try {
    const a = await answer(text);
    const p = clean(a.params);
    const mode = a.match !== "all" ? { match: a.match } : {};
    const plain = a.reading.matched.length === 0 && !a.relaxed && (p.q || "") === text.toLowerCase();
    go("/search", plain ? { q: text } : { ...p, ...mode, nl: text });
  } catch {
    answers.delete(text);
    go("/search", { q: text });
  } finally {
    form?.classList.remove("busy");
  }
}

const FIELD_LABELS = {
  federation: "Registered by",
  country: "Country",
  region: "Region",
  category: "Category",
  availability: "Availability",
  environment: "Environment",
  sharing: "Open to",
  attribute: "Requests",
  source: "Seen in",
  provider_type: "Provider type",
};

function readingBanner(params, a) {
  const reading = a?.reading;
  const terms = (reading?.matched || []).map((m) =>
    h("span", { class: "chip" }, `${FIELD_LABELS[m.field] || m.field}: `, h("b", {}, m.field === "country" ? `${flag(m.value)} ${m.label}` : m.label)),
  );
  if (reading?.params.q) terms.push(h("span", { class: "chip" }, "words: ", h("b", {}, reading.params.q)));
  const byModel = reading && reading.engine !== "rules";
  return h(
    "div",
    { class: "reading", role: "note" },
    h(
      "div",
      { class: "reading-head" },
      h("span", { class: "reading-glyph", "aria-hidden": "true" }, "◉"),
      h("span", {}, "Read “", h("b", {}, params.nl), "” as"),
      reading && h("span", { class: "badge", title: byModel ? `Interpreted by ${reading.engine}` : "Interpreted by the catalog's built-in rules" }, byModel ? "Claude" : "rules"),
    ),
    a?.relaxed && h("p", { class: "reading-relaxed" }, a.relaxed),
    reading?.explanation && h("p", { class: "reading-explain" }, reading.explanation),
    terms.length ? h("div", { class: "reading-terms" }, terms) : h("p", { class: "reading-explain" }, "No filters; searching the words as typed."),
    (reading?.notes || []).length > 0 && h("ul", { class: "reading-notes" }, reading.notes.map((n) => h("li", {}, n))),
    h("a", { href: `#/search?q=${encodeURIComponent(params.nl)}`, class: "reading-literal" }, "Search the exact words instead"),
  );
}

// ---------- views ----------

async function landing() {
  document.body.classList.add("landing");
  // Three examples, different on each visit, so every kind of search gets shown
  // without crowding the box. The first three stand in until the list arrives.
  const tryLink = (ex) => h("a", { href: "#", onclick: (e) => (e.preventDefault(), (input.value = ex), interpretAndGo(ex, form)) }, ex);
  const tryRow = h(
    "div",
    { class: "quick try" },
    h("span", {}, "try:"),
    ["journals not in InCommon", "test instances of Canvas", "services anyone in eduGAIN can use"].map(tryLink),
  );
  examples().then((list) => {
    if (list.length < 3) return;
    const picks = [...list].sort(() => Math.random() - 0.5).slice(0, 3);
    fill(tryRow, h("span", {}, "try:"), picks.map((ex) => tryLink(ex.query)));
  });
  const canvas = h("canvas", { "aria-hidden": "true" });
  const input = h("input", {
    type: "search",
    name: "q",
    placeholder: "Ask in plain words: “R&S services in Sweden that request email”",
    "aria-label": "Search services",
    autocomplete: "off",
  });
  const hint = h("div", { class: "hint" }, "reading the federation…");
  const form = h(
    "form",
    { class: "searchbox", role: "search", onsubmit: (e) => (e.preventDefault(), interpretAndGo(input.value, e.currentTarget)) },
    input,
    h("button", { class: "btn primary", type: "submit" }, "Search"),
  );
  render(
    h(
      "section",
      { class: "network" },
      canvas,
      h(
        "div",
        { class: "portal" },
        h("p", { class: "eyebrow" }, "InCommon and eduGAIN, in one catalog"),
        h("h1", {}, "Find services you can sign in to with your institutional account"),
        form,
        hint,
        h(
          "div",
          { class: "quick" },
          h("a", { href: "#/search?category=research-and-scholarship" }, "Research & Scholarship"),
          h("a", { href: "#/search?category=code-of-conduct-v2" }, "Code of Conduct"),
          h("a", { href: "#/search?availability=incommon-and-edugain" }, "InCommon → eduGAIN"),
          h("a", { href: "#/search?availability=edugain-only" }, "Beyond InCommon"),
          h("a", { href: "#/providers" }, "By provider"),
        ),
        tryRow,
      ),
    ),
  );
  globe = mountNetwork(canvas, { portal: canvas.parentElement.querySelector(".portal") });
  input.focus({ preventScroll: true });

  try {
    const [m, feds] = await Promise.all([meta(), api("/v1/federations")]);
    hint.textContent = `${fmt(m.counts.services)} services · ${fmt(m.counts.providers)} providers · ${fmt(feds.filter((f) => f.listed_in_edugain).length)} federations`;
    globe?.setFederations(
      feds
        .filter((f) => (f.listed_in_edugain || f.code === "INCOMMON") && f.catalog_services + f.catalog_identity_providers > 0)
        .map((f) => ({
          code: f.code,
          name: f.name,
          countries: f.country_codes,
          services: f.catalog_services,
          idps: f.catalog_identity_providers,
        })),
    );
  } catch (e) {
    hint.textContent = "The catalog API is unreachable right now.";
  }
}

const FACET_ORDER = [
  ["availability", "Availability"],
  ["environment", "Environment"],
  ["sharing", "Open to"],
  ["category", "Category"],
  ["federation", "Registered by"],
  ["region", "Region"],
  ["country", "Country"],
  ["attribute", "Requested attribute"],
  ["source", "Seen in"],
  ["provider_type", "Provider type"],
];

async function search(params) {
  setNav("search");
  const q = params.q || "";
  const input = h("input", {
    type: "search",
    value: params.nl || q,
    placeholder: "Search by name, or ask in plain words",
    "aria-label": "Search services",
  });
  // Changing a filter by hand ends the natural-language reading.
  const refine = (changes) => go("/search", { ...params, nl: "", match: params.nl ? "" : params.match, page: "", ...changes });
  const facetsEl = h("aside", { class: "facets", "aria-label": "Filters" });
  const results = h("section", { "aria-live": "polite" }, loading());
  const banner = params.nl ? readingBanner(params, null) : null;
  render(
    h(
      "div",
      { class: "search-head" },
      h(
        "form",
        { class: "searchbox", role: "search", onsubmit: (e) => (e.preventDefault(), interpretAndGo(input.value, e.currentTarget)) },
        input,
        h("button", { class: "btn primary", type: "submit" }, "Search"),
      ),
      h("button", { class: "btn filters-toggle", type: "button", onclick: () => facetsEl.classList.toggle("open") }, "Filters"),
    ),
    // The reading sits above the results in the same column; the grid has exactly two children.
    h("div", { class: "layout" }, facetsEl, h("div", { class: "results-col" }, banner, results)),
  );

  let page, feds;
  try {
    const { nl, ...query } = params;
    [page, feds] = await Promise.all([
      api("/v1/services", { ...query, per_page: 24 }),
      params.federation ? fedIndex() : null,
      nl &&
        answer(nl)
          .then((r) => banner.replaceWith(readingBanner(params, r)))
          .catch(() => {}),
    ]);
  } catch (e) {
    fill(results, failure(e));
    return;
  }

  // Facets.
  for (const [key, label] of FACET_ORDER) {
    const values = page.facets[key] || [];
    if (!values.length) continue;
    const active = params[key];
    const selected = listOf(active);
    const same = key === "federation" ? (a, b) => fedKey(feds, a) === fedKey(feds, b) : (a, b) => a === b;
    const list = h("div", {});
    const limit = 8;
    values.forEach((v, i) => {
      const on = key in MULTI ? selected.some((a) => same(a, v.value)) : active === v.value;
      const btn = h(
        "button",
        {
          type: "button",
          "aria-pressed": on ? "true" : "false",
          hidden: i >= limit && !on,
          onclick: () => {
            let next = on ? "" : v.value;
            if (key in MULTI) next = (on ? selected.filter((a) => !same(a, v.value)) : [...selected, v.value]).join(",");
            refine({ [key]: next });
          },
        },
        h("span", {}, key === "country" ? `${flag(v.value)} ${v.label}` : key === "region" ? bare(v.label) : key === "category" ? v.label : AVAILABILITY[v.value] || v.label),
        h("span", { class: "n" }, fmt(v.count)),
      );
      list.append(btn);
    });
    const more =
      values.length > limit &&
      h(
        "button",
        {
          class: "more",
          type: "button",
          onclick: (e) => {
            list.querySelectorAll("button[hidden]").forEach((b) => b.removeAttribute("hidden"));
            e.target.remove();
          },
        },
        `+ ${values.length - limit} more`,
      );
    const mode = key in MULTI && selected.length > 1 && h("span", { class: "mode" }, `matching ${MULTI[key]}`);
    facetsEl.append(h("div", { class: "facet" }, h("h3", {}, label, mode), list, more));
  }

  // Active filters as removable chips, labeled the way the facets label them.
  const chipLabel = (k, v) =>
    k === "availability"
      ? AVAILABILITY[v] || v
      : k === "environment" || k === "sharing"
        ? (k === "environment" ? ENVIRONMENT : SHARING)[v] || v
        : k === "category"
          ? CATEGORY_LABELS[v] || v
          : (k === "federation" && feds?.get(v.toLowerCase())?.name) ||
            (page.facets[k] || []).find((f) => f.value.toLowerCase() === v.toLowerCase())?.label ||
            v;
  const remove = (k, v) => () => refine({ [k]: k in MULTI ? listOf(params[k]).filter((x) => x !== v).join(",") : "" });
  const chips = FACET_ORDER.filter(([k]) => params[k]).flatMap(([k, label]) =>
    (k in MULTI ? listOf(params[k]) : [params[k]]).map((v) => {
      const region = k === "region" && (page.regions || []).find((r) => r.key === v.toLowerCase());
      if (!region) return h("button", { class: "chip", type: "button", onclick: remove(k, v) }, `${label}: ${chipLabel(k, v)} ×`);
      // A region is one chip that opens to the countries it stood for, and where that list comes from.
      return h(
        "span",
        { class: "chip region-chip" },
        h(
          "details",
          {},
          h("summary", {}, `${label}: ${bare(region.label)} · ${region.countries.length} countries`),
          h(
            "div",
            { class: "region-list" },
            region.countries.map((c) => h("span", { title: c }, `${flag(c)} ${c}`)),
            h(
              "p",
              { class: "region-source" },
              region.as_of && region.kind !== "geographic" ? `Membership as of ${region.as_of}. ` : "",
              region.note ? `${region.note} ` : "",
              Object.keys(region.left || {}).length
                ? `Former members: ${Object.entries(region.left).map(([cc, d]) => `${flag(cc)} ${cc} (left ${d})`).join(", ")}. `
                : "",
              h("a", { href: region.url, target: "_blank", rel: "noopener noreferrer" }, region.source),
              `, checked ${region.checked}.`,
            ),
          ),
        ),
        h("button", { class: "chip-x", type: "button", "aria-label": `Remove ${label}: ${bare(region.label)}`, onclick: remove(k, v) }, "×"),
      );
    }),
  );
  if (params.provider) chips.push(h("button", { class: "chip", type: "button", onclick: () => refine({ provider: "" }) }, "Provider filter ×"));

  const sort = h(
    "select",
    { "aria-label": "Sort", onchange: (e) => go("/search", { ...params, sort: e.target.value, page: "" }) },
    [["", q ? "Relevance" : "Name"], ["name", "Name"], ["provider", "Provider"]].map(([v, l]) =>
      h("option", { value: v, selected: (params.sort || "") === v }, l),
    ),
  );
  const first = (page.page - 1) * page.per_page + 1;
  const last = Math.min(page.total, page.page * page.per_page);
  const pages = Math.max(1, Math.ceil(page.total / page.per_page));
  fill(results, 
    chips.length ? h("div", { class: "active-filters" }, chips) : "",
    h("div", { class: "resultbar" }, h("span", {}, page.total ? `${fmt(first)}–${fmt(last)} of ${fmt(page.total)} services` : ""), sort),
    page.items.length
      ? h("div", { class: "grid" }, page.items.map(serviceCard))
      : h(
          "div",
          { class: "empty" },
          "Nothing down here. Try fewer words or filters",
          params.nl ? [", or ", h("a", { href: `#/search?q=${encodeURIComponent(params.nl)}` }, "search the exact words"), "."] : ".",
        ),
    pages > 1 &&
      h(
        "nav",
        { class: "pager", "aria-label": "Pages" },
        h("button", { class: "btn", disabled: page.page <= 1, onclick: () => go("/search", { ...params, page: page.page - 1 }) }, "← Prev"),
        h("span", {}, `page ${page.page} of ${fmt(pages)}`),
        h("button", { class: "btn", disabled: page.page >= pages, onclick: () => go("/search", { ...params, page: page.page + 1 }) }, "Next →"),
      ),
  );
}

// "Test, from "test" in the entityID's host", "Production, as Claude read it": the
// value with what decided it, so a generated reading is never shown as published.
function environmentNote(env) {
  if (!env) return "unknown";
  const label = ENVIRONMENT[env.value] || env.value;
  if (env.source === "metadata") return `${label}, from ${env.evidence}`;
  if (env.source === "generated") return [h("span", { class: "gen-mark", "aria-label": "Generated:" }, "✦ "), `${label}, as Claude read this service's metadata`];
  return "Unknown: nothing in the metadata marks it, and there is no summary to go by";
}

// The label, why, and who decided it; a generated reading is marked and dated, and
// "any institution" always comes with what it does not promise.
function sharingNote(sh) {
  if (!sh || sh.value === "unknown") return "Unknown: the metadata does not say, and Claude has not read it yet";
  const cluster = sh.signals?.tenant_cluster;
  return [
    sh.source === "generated" && h("span", { class: "gen-mark", "aria-label": "Generated:" }, "✦ "),
    h("b", {}, SHARING[sh.value]),
    sh.reason ? `. ${sh.reason.replace(/\.$/, "")}.` : ".",
    sh.source === "generated" && h("span", { class: "gen-note" }, ` Claude's reading (${sh.model}, ${(sh.generated_at || "").slice(0, 10)}); may be wrong.`),
    cluster && sh.source === "generated" && h("div", { class: "mono", style: "color:var(--muted)" }, `One of ${fmt(cluster.size)} registrations at ${cluster.pattern}`),
    sh.value === "multilateral" && h("div", { style: "color:var(--muted);font-size:12px;margin-top:4px" }, SHARING_CAVEAT),
  ];
}

function langList(map) {
  return Object.entries(map || {}).map(([lang, v]) => ({ lang, v }));
}

function pick(map, lang = "en") {
  if (!map) return null;
  return map[lang] || map.en || map[""] || Object.values(map)[0] || null;
}

async function serviceView(id) {
  setNav("search");
  render(loading());
  let d;
  try {
    d = await api(`/v1/services/${encodeURIComponent(id)}`);
  } catch (e) {
    return render(failure(e));
  }
  if (!d) return render(h("div", { class: "empty" }, "No service with that ID."));
  const s = d.service;
  document.title = `${d.name} · InCatalog`;
  // The service's own metadata logo when one works; otherwise the one ingest found on
  // the organization's website (recorded separately, with where it came from).
  const metaLogo = (s.logos || []).find((l) => !l.broken && safeImg(l.url));
  const siteLogo = !metaLogo && s.discovered_logo && safeImg(s.discovered_logo.url) ? s.discovered_logo : null;
  const logo = metaLogo?.url || siteLogo?.url;
  const siteHost = siteLogo && safeUrl(siteLogo.page) ? new URL(siteLogo.page).host : null;
  const desc = pick(s.descriptions) || pick(s.service_descriptions);
  const contacts = s.contacts || [];
  const fed = d.federation;

  const linkRow = (label, url) => url && safeUrl(url) && h("li", {}, h("a", { href: url, target: "_blank", rel: "noopener noreferrer" }, label), " ↗");

  render(
    h("div", { class: "crumbs" }, h("a", { href: "#/search" }, "services"), " / ", h("a", { href: `#/providers/${d.provider.id}` }, d.provider.name)),
    h(
      "div",
      { class: "hero" },
      logoEl(logo, d.name, true),
      h(
        "div",
        { style: "min-width:0" },
        h("h1", {}, d.name),
        h("div", { class: "sub" }, pick(s.organization.display_names) || pick(s.organization.names) || ""),
        h("div", { class: "mono", style: "color:var(--muted);margin-top:4px" }, s.entity_id),
        siteLogo &&
          h(
            "div",
            { class: "logo-note" },
            (s.logos || []).length ? "The metadata logo doesn't load, so this one is from " : "No logo in the metadata, so this one is from ",
            siteHost ? h("a", { href: siteLogo.page, target: "_blank", rel: "noopener noreferrer" }, siteHost) : "the organization's website",
            ".",
          ),
      ),
    ),
    h(
      "div",
      { class: "cols" },
      h(
        "div",
        {},
        s.curated &&
          h(
            "section",
            { class: "panel curated" },
            h("h2", {}, "Curated (not from metadata)"),
            s.curated.summary && h("p", {}, s.curated.summary),
            h("div", { class: "badges" }, s.curated.provider_type && h("span", { class: "badge cur" }, s.curated.provider_type), (s.curated.tags || []).map((t) => h("span", { class: "badge" }, t))),
          ),
        h(
          "section",
          { class: "panel" },
          h("h2", {}, "About"),
          desc ? h("p", {}, desc) : h("p", { style: "color:var(--muted)" }, "This service publishes no description in its metadata."),
          langList(s.display_names).length > 1 &&
            h("p", { class: "mono", style: "color:var(--muted)" }, langList(s.display_names).map(({ lang, v }) => `${lang || "—"}: ${v}`).join(" · ")),
          (s.keywords || []).length > 0 && h("div", { class: "badges" }, s.keywords.map((k) => h("span", { class: "badge" }, k))),
        ),
        // Written by Claude during ingest; kept visibly apart from what the registrant published.
        s.enrichment &&
          (s.enrichment.summary || s.enrichment.keywords.length > 0) &&
          h(
            "section",
            { class: "panel generated" },
            h("h2", {}, "✦ Summary · written by Claude"),
            s.enrichment.summary && h("p", {}, s.enrichment.summary),
            s.enrichment.keywords.length > 0 &&
              h(
                "div",
                { class: "badges" },
                s.enrichment.keywords.map((k) => h("a", { class: "badge", href: `#/search?q=${encodeURIComponent(k)}` }, k)),
              ),
            h(
              "p",
              { class: "gen-note" },
              BASIS_NOTES[s.enrichment.basis] || "",
              ` Generated ${s.enrichment.generated_at.slice(0, 10)} by ${s.enrichment.model}; may be wrong.`,
            ),
          ),
        h(
          "section",
          { class: "panel" },
          h("h2", {}, "Availability"),
          h("p", {}, h("span", { class: "badge avail" }, AVAILABILITY[s.availability] || s.availability), " ", AVAILABILITY_HELP[s.availability] || ""),
          h(
            "dl",
            { class: "kv" },
            h("dt", {}, "Registered by"),
            h("dd", {}, fed ? `${flag(fed.country_codes[0])} ${fed.name} (${fed.code})` : s.registration.authority || "unknown"),
            s.registration.instant && [h("dt", {}, "Registered"), h("dd", {}, s.registration.instant.slice(0, 10))],
            h("dt", {}, "Seen in"),
            h("dd", {}, s.sources.map((x) => (x === "incommon" ? "InCommon aggregate" : "eduGAIN aggregate")).join(", ")),
            h("dt", {}, "Environment"),
            h("dd", {}, environmentNote(d.environment)),
            h("dt", {}, "Open to"),
            h("dd", {}, sharingNote(d.sharing)),
            s.also_registered_by?.length > 0 && [h("dt", {}, "Also registered by"), h("dd", {}, s.also_registered_by.join(", "))],
          ),
        ),
        h(
          "section",
          { class: "panel" },
          h("h2", {}, "Categories and certifications"),
          s.categories.length
            ? h(
                "ul",
                { class: "linklist" },
                s.categories.map((c) => h("li", {}, h("a", { href: `#/search?category=${encodeURIComponent(c)}` }, CATEGORY_LABELS[c] || c))),
              )
            : h("p", { style: "color:var(--muted)" }, "No entity categories or assurance certifications."),
          (s.other_entity_attributes || []).length > 0 &&
            h(
              "details",
              {},
              h("summary", { class: "mono" }, "other entity attributes"),
              h("ul", { class: "mono" }, s.other_entity_attributes.map((a) => h("li", {}, `${a.name} = ${a.values.join(", ")}`))),
            ),
        ),
        h(
          "section",
          { class: "panel" },
          h("h2", {}, `Requested attributes (${(s.requested_attributes || []).length})`),
          (s.requested_attributes || []).length
            ? h(
                "div",
                { class: "table-wrap" },
                h(
                  "table",
                  {},
                  h("thead", {}, h("tr", {}, h("th", {}, "Attribute"), h("th", {}, "Name"), h("th", {}, "Required"))),
                  h(
                    "tbody",
                    {},
                    s.requested_attributes.map((a) =>
                      h(
                        "tr",
                        {},
                        h("td", {}, h("a", { href: `#/search?attribute=${encodeURIComponent(a.name)}` }, a.friendly_name || "—")),
                        h("td", { class: "mono" }, a.name),
                        h("td", {}, a.required ? "yes" : "no"),
                      ),
                    ),
                  ),
                ),
              )
            : h("p", { style: "color:var(--muted)" }, "None listed in metadata. Many services request attributes through entity categories instead."),
        ),
        (s.notes || []).length > 0 &&
          h("section", { class: "panel" }, h("h2", {}, "Notes on this metadata"), h("ul", { class: "notes" }, s.notes.map((n) => h("li", {}, n)))),
      ),
      h(
        "aside",
        {},
        h(
          "section",
          { class: "panel" },
          h("h2", {}, "Links"),
          h(
            "ul",
            { class: "linklist" },
            linkRow("Service information", pick(s.information_urls)),
            linkRow("Privacy statement", pick(s.privacy_statement_urls)),
            linkRow("Organization website", pick(s.organization.urls)),
            linkRow("Live metadata (InCommon MDQ)", d.links.mdq),
            linkRow("All federations publishing it (REFEDS MET)", d.links.met),
            linkRow("eduGAIN entity record", d.links.edugain),
          ),
        ),
        contacts.length > 0 &&
          h(
            "section",
            { class: "panel" },
            h("h2", {}, "Contacts"),
            h(
              "dl",
              { class: "kv" },
              contacts.map((c) => [h("dt", {}, c.kind), h("dd", {}, c.emails.map((e, i) => [i ? ", " : "", h("a", { href: `mailto:${e}` }, e)]))]),
            ),
          ),
        h(
          "section",
          { class: "panel" },
          h("h2", {}, "Technical"),
          h(
            "dl",
            { class: "kv" },
            h("dt", {}, "Signing key"),
            h("dd", { class: s.has_signing_key ? "yes" : "no" }, s.has_signing_key ? "yes" : "no"),
            h("dt", {}, "Encryption key"),
            h("dd", { class: s.has_encryption_key ? "yes" : "no" }, s.has_encryption_key ? "yes" : "no"),
            h("dt", {}, "Discovery response"),
            h("dd", {}, s.supports_discovery_response ? "yes" : "no"),
            h("dt", {}, "NameID formats"),
            h("dd", { class: "mono" }, (s.name_id_formats || []).map((f) => f.split(":").pop()).join(", ") || "—"),
            h("dt", {}, "ACS bindings"),
            h("dd", { class: "mono" }, (s.acs_bindings || []).map((b) => b.split(":").pop()).join(", ") || "—"),
            h("dt", {}, "Catalog ID"),
            h("dd", { class: "mono" }, s.id),
          ),
        ),
        h(
          "section",
          { class: "panel" },
          h("h2", {}, "Provider"),
          h("p", {}, h("a", { href: `#/providers/${d.provider.id}` }, d.provider.name)),
          h("p", { class: "mono", style: "color:var(--muted)" }, `${fmt(d.provider.service_count)} service${d.provider.service_count === 1 ? "" : "s"} registered under this organization name`),
        ),
      ),
    ),
  );
}

async function providersView(params) {
  setNav("providers");
  const input = h("input", { type: "search", value: params.q || "", placeholder: "Filter providers", "aria-label": "Filter providers" });
  const list = h("div", { class: "rows", "aria-live": "polite" }, loading());
  render(
    h("h1", { class: "page-title" }, "Providers"),
    h(
      "p",
      { class: "lede" },
      "Services grouped by the organization name each registrant chose. Vendors that register one service per customer show up here as a single provider with many services. Names are free text, so parent and subsidiary organizations are not merged.",
    ),
    h(
      "div",
      { class: "search-head" },
      h("form", { class: "searchbox", onsubmit: (e) => (e.preventDefault(), go("/providers", { q: input.value.trim() })) }, input, h("button", { class: "btn primary" }, "Filter")),
    ),
    list,
  );
  try {
    const page = await api("/v1/providers", { q: params.q, page: params.page, per_page: 50, sort: params.sort });
    const pages = Math.max(1, Math.ceil(page.total / page.per_page));
    fill(list, 
      ...page.items.map((p) =>
        h(
          "a",
          { class: "row", href: `#/providers/${p.id}` },
          h(
            "div",
            {},
            h("div", { style: "font-weight:600" }, p.name),
            h(
              "div",
              { class: "meta" },
              [p.federations.slice(0, 3).join(", "), Object.keys(p.categories).filter((c) => !QUIET_CATEGORIES.has(c)).map((c) => CATEGORY_LABELS[c] || c).join(" · ")]
                .filter(Boolean)
                .join(" — "),
            ),
          ),
          h("div", { class: "count", title: "services" }, fmt(p.service_count)),
        ),
      ),
      page.items.length ? "" : h("div", { class: "empty" }, "No providers match."),
      pages > 1 &&
        h(
          "nav",
          { class: "pager" },
          h("button", { class: "btn", disabled: page.page <= 1, onclick: () => go("/providers", { ...params, page: page.page - 1 }) }, "← Prev"),
          h("span", {}, `page ${page.page} of ${fmt(pages)} · ${fmt(page.total)} providers`),
          h("button", { class: "btn", disabled: page.page >= pages, onclick: () => go("/providers", { ...params, page: page.page + 1 }) }, "Next →"),
        ),
    );
  } catch (e) {
    fill(list, failure(e));
  }
}

async function providerView(id) {
  setNav("providers");
  render(loading());
  let d;
  try {
    d = await api(`/v1/providers/${encodeURIComponent(id)}`);
  } catch (e) {
    return render(failure(e));
  }
  if (!d) return render(h("div", { class: "empty" }, "No provider with that ID."));
  const p = d.provider;
  document.title = `${p.name} · InCatalog`;
  render(
    h("div", { class: "crumbs" }, h("a", { href: "#/providers" }, "providers"), " / ", p.name),
    h("h1", { class: "page-title" }, p.name),
    h(
      "p",
      { class: "lede" },
      `${fmt(p.service_count)} service${p.service_count === 1 ? "" : "s"}`,
      p.registrars.length ? ` · registered by ${p.federations.join(", ") || p.registrars.join(", ")}` : "",
      p.urls[0] && safeUrl(p.urls[0]) ? [" · ", h("a", { href: p.urls[0], target: "_blank", rel: "noopener noreferrer" }, "website ↗")] : "",
    ),
    h("div", { class: "grid" }, d.services.map(serviceCard)),
  );
}

async function federationsView(params) {
  setNav("federations");
  render(loading());
  let feds;
  try {
    feds = await api("/v1/federations");
  } catch (e) {
    return render(failure(e));
  }
  const key = params.sort || "catalog_services";
  const sorted = [...feds].sort((a, b) => (key === "name" ? a.name.localeCompare(b.name) : (b[key] ?? 0) - (a[key] ?? 0)));
  const th = (k, label, num) => h("th", { class: num ? "num" : "" }, h("button", { onclick: () => go("/federations", { sort: k }) }, label, key === k ? " ↓" : ""));
  render(
    h("h1", { class: "page-title" }, "Federations"),
    h(
      "p",
      { class: "lede" },
      "Every federation in the eduGAIN database, joined to this catalog by registration authority. Service counts are what this catalog holds from the InCommon and eduGAIN aggregates; eduGAIN's own reported figures are shown alongside.",
    ),
    h(
      "div",
      { class: "table-wrap" },
      h(
        "table",
        {},
        h("thead", {}, h("tr", {}, th("name", "Federation"), h("th", {}, "Country"), h("th", {}, "Status"), th("catalog_services", "Services", true), th("catalog_identity_providers", "IdPs", true), h("th", { class: "num" }, "eduGAIN SPs"), h("th", {}, "Policy"))),
        h(
          "tbody",
          {},
          sorted.map((f) =>
            h(
              "tr",
              {},
              h(
                "td",
                {},
                f.catalog_services ? h("a", { href: `#/search?federation=${encodeURIComponent(f.registration_authority)}` }, f.name) : f.name,
                h("div", { class: "mono", style: "color:var(--muted)" }, f.code || f.registration_authority),
              ),
              h("td", {}, (f.country_codes || []).map((c) => `${flag(c)} ${c}`).join(" ")),
              h("td", {}, f.listed_in_edugain ? f.status : "not in eduGAIN list"),
              h("td", { class: "num" }, fmt(f.catalog_services)),
              h("td", { class: "num" }, fmt(f.catalog_identity_providers)),
              h("td", { class: "num" }, f.edugain_reported?.sps != null ? fmt(f.edugain_reported.sps) : "—"),
              h(
                "td",
                {},
                (f.policy_urls || []).slice(0, 1).map((u) => safeUrl(u) && h("a", { href: u, target: "_blank", rel: "noopener noreferrer" }, "policy ↗")),
                (f.registration_practice_urls || []).slice(0, 1).map((u) => safeUrl(u) && [" ", h("a", { href: u, target: "_blank", rel: "noopener noreferrer" }, "MRPS ↗")]),
              ),
            ),
          ),
        ),
      ),
    ),
  );
}

// What each filter matches and how they combine, for people rather than API clients.
const FILTER_HELP = [
  ["Registered by", "The federation that registered the service. Pick several and a service registered by any of them matches."],
  ["Country and region", "The registering federation's country. A region or grouping stands for its countries, and countries and regions together are one set of places: a service in any of them matches. A federation that lists several countries is in a region when any of them is."],
  ["Category", "Entity categories and certifications, such as Research & Scholarship or Sirtfi. A service must carry every one you pick."],
  ["Availability", "Where the service can be used from: InCommon only, exported to eduGAIN, or registered elsewhere and imported."],
  ["Environment", "Production, test, staging, or development. It is read from markers in the entityID or the name (test., -dev, \u201c(Staging)\u201d), or else from Claude\u2019s reading of the metadata (marked \u2726). Every environment is shown unless you pick some; test and staging copies rank below the live service they mirror."],
  ["Open to", `Any institution (multilateral) or one institution (bilateral). Per-campus tenants such as <campus>.zoom.us are decided from the metadata; most other labels are Claude\u2019s reading (marked \u2726). ${SHARING_CAVEAT}`],
  ["Requests", "A user attribute the service asks identity providers for, such as email or ORCID."],
  ["Seen in", "Which aggregate the service appears in: InCommon\u2019s or eduGAIN\u2019s."],
];

function howToSearch(regions, exs) {
  const kinds = { geographic: "UN M49 region", political: "Grouping", network: "Research network" };
  return h(
    "section",
    { class: "panel" },
    h("h2", {}, "How to search"),
    h(
      "p",
      {},
      "Type a service\u2019s name, or ask in plain words. Above the results, the page shows how it read your search, and you can search the exact words instead. Different filters combine: a service must match all of them.",
    ),
    h("dl", { class: "kv" }, FILTER_HELP.map(([k, v]) => [h("dt", {}, k), h("dd", {}, v)])),
    exs.length > 0 &&
      h(
        "div",
        {},
        h("h3", {}, "Searches it understands"),
        h(
          "ul",
          { class: "linklist" },
          exs.map((ex) => h("li", {}, h("a", { href: "#", onclick: (e) => (e.preventDefault(), interpretAndGo(ex.query)) }, ex.query), h("span", { style: "color:var(--muted)" }, ` \u2014 ${ex.shows}`))),
        ),
      ),
    h(
      "p",
      { class: "gen-note" },
      "\u2726 marks text Claude generated from a service\u2019s metadata: summaries, some environments, and most \u201copen to\u201d labels. Each says which model wrote it and when, and it may be wrong.",
    ),
    regions.length > 0 &&
      h(
        "details",
        {},
        h("summary", {}, `Regions and groupings (${regions.length})`),
        h(
          "p",
          {},
          "Each stands for a list of countries with its source. Geographic regions follow UN M49. Groupings say when their membership took effect, and every list says when it was last checked against its source. Contested lists are left out on purpose.",
        ),
        h(
          "div",
          { class: "table-wrap" },
          h(
            "table",
            {},
            h("thead", {}, h("tr", {}, h("th", {}, "Region"), h("th", {}, "Kind"), h("th", { class: "num" }, "Countries"), h("th", {}, "Source"), h("th", {}, "Membership as of"), h("th", {}, "Checked"))),
            h(
              "tbody",
              {},
              regions.map((r) =>
                h(
                  "tr",
                  {},
                  h("td", {}, h("a", { href: `#/search?region=${encodeURIComponent(r.key)}` }, bare(r.label)), r.note && h("div", { style: "color:var(--muted);font-size:12px" }, r.note)),
                  h("td", {}, kinds[r.kind] || r.kind),
                  h("td", { class: "num" }, fmt(r.countries.length)),
                  h("td", {}, h("a", { href: r.url, target: "_blank", rel: "noopener noreferrer" }, r.source)),
                  h("td", {}, r.kind === "geographic" ? "\u2014" : r.as_of || "\u2014"),
                  h("td", {}, r.checked),
                ),
              ),
            ),
          ),
        ),
      ),
  );
}

async function aboutView() {
  setNav("about");
  render(loading());
  let m, cats, regions, exs;
  try {
    [m, cats, regions, exs] = await Promise.all([meta(), api("/v1/categories"), api("/v1/regions").catch(() => []), examples()]);
  } catch (e) {
    return render(failure(e));
  }
  render(
    h("h1", { class: "page-title" }, "About this catalog"),
    h(
      "p",
      { class: "lede" },
      "InCatalog lists the SAML service providers published in the InCommon and eduGAIN metadata aggregates. Everything except fields marked curated is derived from that metadata; the federation list comes from the eduGAIN database.",
    ),
    howToSearch(regions || [], exs || []),
    h(
      "section",
      { class: "panel" },
      h("h2", {}, "Snapshot"),
      h(
        "dl",
        { class: "kv" },
        h("dt", {}, "Generated"),
        h("dd", {}, m.generated_at),
        h("dt", {}, "Services"),
        h("dd", {}, `${fmt(m.counts.services)} (${fmt(m.counts.incommon_registered_services)} registered by InCommon)`),
        h("dt", {}, "Providers"),
        h("dd", {}, fmt(m.counts.providers)),
        h("dt", {}, "Identity providers seen"),
        h("dd", {}, fmt(m.counts.identity_providers)),
      ),
    ),
    h(
      "section",
      { class: "panel" },
      h("h2", {}, "Sources"),
      h(
        "div",
        { class: "table-wrap" },
        h(
          "table",
          {},
          h("thead", {}, h("tr", {}, h("th", {}, "Aggregate"), h("th", {}, "Last-Modified"), h("th", {}, "Valid until"), h("th", { class: "num" }, "Entities"), h("th", { class: "num" }, "SPs"), h("th", {}, "Signature"), h("th", {}, "SHA-256"))),
          h(
            "tbody",
            {},
            m.sources.map((s) =>
              h(
                "tr",
                {},
                h("td", {}, h("a", { href: s.url }, s.url)),
                h("td", {}, s.last_modified || "—"),
                h("td", {}, s.valid_until || "—"),
                h("td", { class: "num" }, fmt(s.entities)),
                h("td", { class: "num" }, fmt(s.service_providers)),
                h("td", { class: s.signature_verified ? "yes" : "no" }, s.signature_verified ? "verified" : "not verified"),
                h("td", { class: "mono", title: s.sha256 || "" }, (s.sha256 || "").slice(0, 12) + "…"),
              ),
            ),
          ),
        ),
      ),
    ),
    h("section", { class: "panel" }, h("h2", {}, "Read with care"), h("ul", { class: "notes" }, m.caveats.map((c) => h("li", {}, c)))),
    m.warnings.length > 0 && h("section", { class: "panel" }, h("h2", {}, "Ingest warnings"), h("ul", { class: "notes" }, m.warnings.map((c) => h("li", {}, c)))),
    h(
      "section",
      { class: "panel" },
      h("h2", {}, "Categories"),
      h(
        "p",
        {},
        "Several categories exist under more than one URI. The catalog treats each set as synonyms and matches whole values only. The full list of raw values is available from ",
        h("a", { href: `${API}/v1/categories` }, "/v1/categories"),
        ".",
      ),
      h(
        "div",
        { class: "table-wrap" },
        h(
          "table",
          {},
          h("thead", {}, h("tr", {}, h("th", {}, "Category"), h("th", {}, "URIs"), h("th", { class: "num" }, "Services"))),
          h(
            "tbody",
            {},
            cats.categories.map((c) =>
              h(
                "tr",
                {},
                h("td", {}, c.spec_url ? h("a", { href: c.spec_url, target: "_blank", rel: "noopener noreferrer" }, c.label) : c.label, h("div", { style: "color:var(--muted);font-size:12px" }, c.description)),
                h("td", { class: "mono" }, c.uris.map((u) => h("div", {}, u))),
                h("td", { class: "num" }, c.service_count ? h("a", { href: `#/search?category=${c.key}` }, fmt(c.service_count)) : "0"),
              ),
            ),
          ),
        ),
      ),
    ),
    h("p", { class: "mono" }, "API: ", h("a", { href: `${API}/openapi.json` }, "OpenAPI document"), " · data refreshes when the federations republish (daily)."),
  );
}

// ---------- router ----------

async function route() {
  globe?.stop();
  globe = null;
  document.body.classList.remove("landing");
  document.title = "InCatalog · Federated Service Catalog";
  const { parts, params } = parseHash();
  const [head, id] = parts;
  window.scrollTo(0, 0);
  if (!head) {
    setNav("");
    return landing();
  }
  if (head === "search") return search(params);
  if (head === "services" && id) return serviceView(id);
  if (head === "providers") return id ? providerView(id) : providersView(params);
  if (head === "federations") return federationsView(params);
  if (head === "about") return aboutView();
  render(h("div", { class: "empty" }, "Lost in the underworld. ", h("a", { href: "#/" }, "Return to the surface.")));
}

window.addEventListener("hashchange", route);
route();

meta()
  .then((m) => {
    const lm = m.sources.map((s) => s.last_modified).filter(Boolean);
    document.getElementById("provenance").textContent =
      `Built from federation metadata · snapshot ${m.generated_at.replace("T", " ").replace("Z", " UTC")}` + (lm.length ? ` · aggregates published ${lm[0]}` : "");
  })
  .catch(() => {
    document.getElementById("provenance").textContent = "Catalog API unreachable";
  });
