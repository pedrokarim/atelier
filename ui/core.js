"use strict";

/*
  Noyau commun à toutes les pages : dialogue avec l'hôte, registre des
  modules et des widgets, mise en forme, et petits composants (infobulle,
  menus, boîtes de dialogue).

  Le contrat d'un widget, côté page :

    Atelier.widget({
      id: "markets.quote",          // préfixé par l'identifiant du module
      module: "markets",
      title: "Bourse",
      description: "Cours d'une action, mis à jour en continu.",
      sizes: ["s", "m", "l"],       // tailles proposées ; la première est la taille par défaut
      refresh: 30,                  // secondes entre deux rendus (0 : jamais)
      events: ["markets.update"],   // événements de l'hôte qui provoquent un rendu
      async render(el, ctx) {       // appelé au montage puis à chaque rafraîchissement
        const quote = await ctx.request("quote", { symbol: "AAPL" });
        el.innerHTML = `…`;
      },
    });

  `ctx` offre : `size`, `settings`, `request(method, params)` (vers la moitié
  Rust du module), `open(route, params)` (ouvre une page dans la fenêtre
  principale) et `tip(element, html)` (infobulle).
*/

const Atelier = (() => {
  const boot = window.ATELIER_BOOT || {};
  const pending = new Map();
  const listeners = new Map();
  const modules = new Map();
  const widgets = new Map();
  const failures = [];
  let nextRequest = 1;

  /* --- Dialogue avec l'hôte --------------------------------------------- */

  function request(target, method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = nextRequest++;
      pending.set(id, { resolve, reject });
      window.chrome.webview.postMessage(
        JSON.stringify({ kind: "request", id, target, method, params }));
    });
  }

  function resolve(id, ok, value) {
    const entry = pending.get(id);
    if (!entry) return;
    pending.delete(id);
    if (ok) entry.resolve(value);
    else entry.reject(new Error(value));
  }

  function on(event, handler) {
    if (!listeners.has(event)) listeners.set(event, new Set());
    listeners.get(event).add(handler);
    return () => listeners.get(event).delete(handler);
  }

  function emit(event, payload) {
    for (const handler of listeners.get(event) || []) {
      try { handler(payload); } catch (error) { console.error(error); }
    }
  }

  /* --- Registre ---------------------------------------------------------- */

  function module(definition) {
    modules.set(definition.id, { pages: [], ...definition });
  }

  function widget(definition) {
    widgets.set(definition.id, {
      sizes: ["m"],
      refresh: 0,
      ...definition,
    });
  }

  function moduleFailed(id, error) {
    console.error(`module ${id}`, error);
    failures.push({ id, message: String(error && error.message || error) });
  }

  /* --- Mise en forme ----------------------------------------------------- */

  const NBSP = " ";

  function escape(value) {
    return String(value ?? "").replace(/[&<>"']/g, (c) => ({
      "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
    })[c]);
  }

  /* « 1 h 05 », « 12 min », « 40 s ». */
  function duration(seconds) {
    const total = Math.max(0, Math.round(seconds || 0));
    if (total < 60) return `${total}${NBSP}s`;
    const minutes = Math.floor(total / 60);
    if (minutes < 60) return `${minutes}${NBSP}min`;
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    return rest ? `${hours}${NBSP}h${NBSP}${String(rest).padStart(2, "0")}` : `${hours}${NBSP}h`;
  }

  /* Même chose en HTML, unités en retrait : pour les grands chiffres. */
  function durationHtml(seconds) {
    const total = Math.max(0, Math.round(seconds || 0));
    const unit = (text) => `<span class="unit">${text}</span>`;
    if (total < 60) return `${total}${unit("s")}`;
    const minutes = Math.floor(total / 60);
    if (minutes < 60) return `${minutes}${unit("min")}`;
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    return `${hours}${unit("h")}${rest ? String(rest).padStart(2, "0") : ""}`;
  }

  /* Secondes depuis minuit, en heure lisible : « 15:57 ». */
  function clock(daySeconds) {
    const total = Math.max(0, Math.round(daySeconds));
    const hours = Math.floor(total / 3600) % 25;
    const minutes = Math.floor((total % 3600) / 60);
    return `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}`;
  }

  function parseDate(text) {
    const [year, month, day] = text.split("-").map(Number);
    return new Date(year, month - 1, day);
  }

  function isoDate(date) {
    const pad = (n) => String(n).padStart(2, "0");
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  }

  function addDays(text, days) {
    const date = parseDate(text);
    date.setDate(date.getDate() + days);
    return isoDate(date);
  }

  const formatters = {
    long: new Intl.DateTimeFormat("fr-FR", { weekday: "long", day: "numeric", month: "long" }),
    short: new Intl.DateTimeFormat("fr-FR", { day: "numeric", month: "short" }),
    weekday: new Intl.DateTimeFormat("fr-FR", { weekday: "short" }),
    weekdayLong: new Intl.DateTimeFormat("fr-FR", { weekday: "long" }),
  };

  function dateLabel(text, style = "long") {
    return formatters[style].format(parseDate(text));
  }

  function capitalize(text) {
    return text ? text[0].toUpperCase() + text.slice(1) : text;
  }

  /* Variation relative, signée, avec espace insécable avant « % ». */
  function delta(current, reference) {
    if (!reference) return null;
    const change = Math.round(((current - reference) / reference) * 100);
    return { change, text: `${change > 0 ? "+" : change < 0 ? "−" : ""}${Math.abs(change)}${NBSP}%` };
  }

  /* --- Icônes ------------------------------------------------------------ */

  const ICON_PATHS = {
    home: '<path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/><path d="M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
    grid: '<rect width="7" height="7" x="3" y="3" rx="1.5"/><rect width="7" height="7" x="14" y="3" rx="1.5"/><rect width="7" height="7" x="14" y="14" rx="1.5"/><rect width="7" height="7" x="3" y="14" rx="1.5"/>',
    settings: '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
    clock: '<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>',
    calendar: '<path d="M8 2v4"/><path d="M16 2v4"/><rect width="18" height="18" x="3" y="4" rx="2"/><path d="M3 10h18"/>',
    chart: '<path d="M3 3v16a2 2 0 0 0 2 2h16"/><path d="M18 17V9"/><path d="M13 17V5"/><path d="M8 17v-3"/>',
    apps: '<rect width="7" height="7" x="3" y="3" rx="1"/><rect width="7" height="7" x="3" y="14" rx="1"/><path d="M14 4h7"/><path d="M14 9h7"/><path d="M14 15h7"/><path d="M14 20h7"/>',
    target: '<circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="2"/>',
    hourglass: '<path d="M5 22h14"/><path d="M5 2h14"/><path d="M17 22v-4.172a2 2 0 0 0-.586-1.414L12 12l-4.414 4.414A2 2 0 0 0 7 17.828V22"/><path d="M7 2v4.172a2 2 0 0 0 .586 1.414L12 12l4.414-4.414A2 2 0 0 0 17 6.172V2"/>',
    sliders: '<line x1="21" x2="14" y1="4" y2="4"/><line x1="10" x2="3" y1="4" y2="4"/><line x1="21" x2="12" y1="12" y2="12"/><line x1="8" x2="3" y1="12" y2="12"/><line x1="21" x2="16" y1="20" y2="20"/><line x1="12" x2="3" y1="20" y2="20"/><line x1="14" x2="14" y1="2" y2="6"/><line x1="8" x2="8" y1="10" y2="14"/><line x1="16" x2="16" y1="18" y2="22"/>',
    plus: '<path d="M5 12h14"/><path d="M12 5v14"/>',
    more: '<circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/>',
    pin: '<path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"/>',
    trash: '<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/>',
    edit: '<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/>',
    check: '<path d="M20 6 9 17l-5-5"/>',
    x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
    chevronLeft: '<path d="m15 18-6-6 6-6"/>',
    chevronRight: '<path d="m9 18 6-6-6-6"/>',
    external: '<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
    layers: '<path d="M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z"/><path d="m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65"/><path d="m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65"/>',
    folder: '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
    file: '<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>',
    power: '<path d="M12 2v10"/><path d="M18.4 6.6a9 9 0 1 1-12.77.04"/>',
    download: '<path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/>',
    pause: '<rect x="14" y="4" width="4" height="16" rx="1"/><rect x="6" y="4" width="4" height="16" rx="1"/>',
    play: '<path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/>',
    eyeOff: '<path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49"/><path d="M14.084 14.158a3 3 0 0 1-4.242-4.242"/><path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143"/><path d="m2 2 20 20"/>',
    eye: '<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/>',
    info: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>',
    alert: '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>',
    ban: '<circle cx="12" cy="12" r="10"/><path d="m4.9 4.9 14.2 14.2"/>',
    search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
    activity: '<path d="M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2"/>',
    monitor: '<rect width="20" height="14" x="2" y="3" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/>',
    arrowUp: '<path d="m5 12 7-7 7 7"/><path d="M12 19V5"/>',
    arrowDown: '<path d="M12 5v14"/><path d="m19 12-7 7-7-7"/>',
    arrowRight: '<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>',
    minus: '<path d="M5 12h14"/>',
    gauge: '<path d="m12 14 4-4"/><path d="M3.34 19a10 10 0 1 1 17.32 0"/>',
    cpu: '<rect width="16" height="16" x="4" y="4" rx="2"/><rect width="6" height="6" x="9" y="9" rx="1"/><path d="M15 2v2"/><path d="M15 20v2"/><path d="M2 15h2"/><path d="M2 9h2"/><path d="M20 15h2"/><path d="M20 9h2"/><path d="M9 2v2"/><path d="M9 20v2"/>',
    memory: '<path d="M6 19v-3"/><path d="M10 19v-3"/><path d="M14 19v-3"/><path d="M18 19v-3"/><path d="M8 11V9"/><path d="M16 11V9"/><path d="M12 11V9"/><path d="M2 15h20"/><path d="M2 7a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v1.1a2 2 0 0 0 0 3.837V17a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-5.1a2 2 0 0 0 0-3.837Z"/>',
    move: '<path d="M12 2v20"/><path d="m15 19-3 3-3-3"/><path d="m19 9 3 3-3 3"/><path d="M2 12h20"/><path d="m5 9-3 3 3 3"/><path d="m9 5 3-3 3 3"/>',
    speaker: '<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z"/><path d="M16 9a5 5 0 0 1 0 6"/><path d="M19.364 18.364a9 9 0 0 0 0-12.728"/>',
    keyboard: '<path d="M10 8h.01"/><path d="M12 12h.01"/><path d="M14 8h.01"/><path d="M16 12h.01"/><path d="M18 8h.01"/><path d="M6 8h.01"/><path d="M7 16h10"/><path d="M8 12h.01"/><rect width="20" height="16" x="2" y="4" rx="2"/>',
    clipboard: '<rect width="8" height="4" x="8" y="2" rx="1" ry="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>',
  };

  function icon(name, size = 16, extra = "") {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" ${extra}>${ICON_PATHS[name] || ""}</svg>`;
  }

  /*
    Petits éléments dessinés plutôt que des caractères Unicode, dont le rendu
    varie d'une police à l'autre : séparateur entre deux informations, et
    marque d'une valeur absente.
  */
  const sep = '<span class="sep" aria-hidden="true"></span>';
  const none = `<span class="none" aria-label="aucune donnée">${icon("minus", 14)}</span>`;

  /* --- Infobulle --------------------------------------------------------- */

  let tooltipNode = null;

  function tooltipElement() {
    if (!tooltipNode) {
      tooltipNode = document.createElement("div");
      tooltipNode.className = "tooltip";
      document.body.appendChild(tooltipNode);
    }
    return tooltipNode;
  }

  function showTooltip(html, x, y) {
    const node = tooltipElement();
    node.innerHTML = html;
    node.classList.add("visible");
    const width = node.offsetWidth;
    const height = node.offsetHeight;
    let left = x + 14;
    let top = y + 14;
    if (left + width > window.innerWidth - 8) left = x - width - 14;
    if (top + height > window.innerHeight - 8) top = y - height - 14;
    node.style.left = `${Math.max(8, left)}px`;
    node.style.top = `${Math.max(8, top)}px`;
  }

  function hideTooltip() {
    if (tooltipNode) tooltipNode.classList.remove("visible");
  }

  /* Attache une infobulle ; `content` peut être une fonction (évaluée au survol). */
  function tip(element, content) {
    element.addEventListener("mousemove", (event) => {
      const html = typeof content === "function" ? content(event) : content;
      if (html) showTooltip(html, event.clientX, event.clientY);
      else hideTooltip();
    });
    element.addEventListener("mouseleave", hideTooltip);
  }

  /* --- Menus ------------------------------------------------------------- */

  let openPopover = null;

  function closePopover() {
    if (openPopover) {
      openPopover.node.remove();
      if (openPopover.anchor) openPopover.anchor.setAttribute("aria-expanded", "false");
      openPopover = null;
    }
  }

  /*
    Menu contextuel. `items` : { label, icon, action, danger, checked } ou
    "separator", ou { label: "…", heading: true }, ou { sizes, current, choose }.
  */
  function popover(anchor, items) {
    closePopover();
    const node = document.createElement("div");
    node.className = "popover";
    node.setAttribute("role", "menu");

    for (const item of items) {
      if (item === "separator") {
        node.insertAdjacentHTML("beforeend", '<div class="popover-sep"></div>');
      } else if (item.heading) {
        node.insertAdjacentHTML("beforeend", `<div class="popover-label">${escape(item.label)}</div>`);
      } else if (item.sizes) {
        const row = document.createElement("div");
        row.className = "popover-sizes";
        for (const size of item.sizes) {
          const button = document.createElement("button");
          button.type = "button";
          button.textContent = sizeLabel(size);
          button.title = sizeName(size);
          button.setAttribute("aria-pressed", String(size === item.current));
          button.addEventListener("click", () => { closePopover(); item.choose(size); });
          row.appendChild(button);
        }
        node.appendChild(row);
      } else {
        const button = document.createElement("button");
        button.type = "button";
        button.className = `popover-item${item.danger ? " danger" : ""}`;
        button.innerHTML = `${item.icon ? icon(item.icon, 15) : ""}<span>${escape(item.label)}</span>${item.checked ? `<span class="check">${icon("check", 15)}</span>` : ""}`;
        button.addEventListener("click", () => { closePopover(); item.action(); });
        node.appendChild(button);
      }
    }

    document.body.appendChild(node);
    const rect = anchor.getBoundingClientRect();
    const width = node.offsetWidth;
    const height = node.offsetHeight;
    let left = rect.right - width;
    let top = rect.bottom + 4;
    if (left < 8) left = Math.min(rect.left, window.innerWidth - width - 8);
    if (top + height > window.innerHeight - 8) top = Math.max(8, rect.top - height - 4);
    node.style.left = `${Math.max(8, left)}px`;
    node.style.top = `${top}px`;

    anchor.setAttribute("aria-expanded", "true");
    openPopover = { node, anchor };
    const first = node.querySelector("button");
    if (first) first.focus({ preventScroll: true });
  }

  document.addEventListener("mousedown", (event) => {
    if (openPopover && !openPopover.node.contains(event.target) && event.target !== openPopover.anchor && !openPopover.anchor.contains(event.target)) {
      closePopover();
    }
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && openPopover) {
      event.stopPropagation();
      closePopover();
    }
  }, true);
  window.addEventListener("blur", () => { closePopover(); hideTooltip(); });

  function sizeLabel(size) {
    return { s: "S", m: "M", l: "L", w: "XL" }[size] || size.toUpperCase();
  }

  function sizeName(size) {
    return { s: "Petit", m: "Moyen", l: "Grand", w: "Bandeau" }[size] || size;
  }

  /* --- Boîtes de dialogue ------------------------------------------------ */

  /*
    Ouvre une boîte de dialogue. `build(body, close)` remplit le corps ;
    `actions` : [{ label, primary, danger, ghost, run(close) }].
  */
  function dialog({ title, subtitle, wide, build, actions = [] }) {
    const backdrop = document.createElement("div");
    backdrop.className = "backdrop";
    backdrop.innerHTML = `
      <div class="modal${wide ? " wide" : ""}" role="dialog" aria-modal="true">
        <div class="modal-head">
          <div class="modal-title">${escape(title)}</div>
          ${subtitle ? `<div class="modal-sub">${escape(subtitle)}</div>` : ""}
        </div>
        <div class="modal-body"></div>
        <div class="modal-actions"></div>
      </div>`;
    const body = backdrop.querySelector(".modal-body");
    const footer = backdrop.querySelector(".modal-actions");

    const close = () => {
      backdrop.remove();
      document.removeEventListener("keydown", onKey, true);
    };
    const onKey = (event) => {
      if (event.key === "Escape") { event.stopPropagation(); close(); }
    };
    document.addEventListener("keydown", onKey, true);
    backdrop.addEventListener("mousedown", (event) => { if (event.target === backdrop) close(); });

    for (const action of actions) {
      if (action.spacer) {
        footer.insertAdjacentHTML("beforeend", '<div class="spacer"></div>');
        continue;
      }
      const button = document.createElement("button");
      button.type = "button";
      button.className = `btn${action.primary ? " btn-primary" : ""}${action.danger ? " btn-danger" : ""}${action.ghost ? " btn-ghost" : ""}`;
      button.innerHTML = `${action.icon ? icon(action.icon, 15) : ""}${escape(action.label)}`;
      button.addEventListener("click", () => action.run ? action.run(close) : close());
      footer.appendChild(button);
    }
    if (!actions.length) footer.remove();

    if (build) build(body, close);
    document.body.appendChild(backdrop);
    const focusable = body.querySelector("input, select, button") || footer.querySelector(".btn-primary");
    if (focusable) focusable.focus();
    return close;
  }

  function confirm(title, body, label = "Confirmer", danger = true) {
    return new Promise((done) => {
      dialog({
        title,
        build: (root) => { root.innerHTML = `<div style="color:var(--text-2);white-space:pre-wrap">${escape(body)}</div>`; },
        actions: [
          { label: "Annuler", ghost: true, run: (close) => { close(); done(false); } },
          { label, primary: !danger, danger, run: (close) => { close(); done(true); } },
        ],
      });
    });
  }

  function toast(text, kind = "info") {
    let host = document.querySelector(".toasts");
    if (!host) {
      host = document.createElement("div");
      host.className = "toasts";
      document.body.appendChild(host);
    }
    const node = document.createElement("div");
    node.className = `toast ${kind}`;
    node.innerHTML = `${icon(kind === "error" ? "alert" : kind === "success" ? "check" : "info", 16)}<div class="toast-body"></div>`;
    node.querySelector(".toast-body").textContent = text;
    host.appendChild(node);
    setTimeout(() => node.remove(), kind === "error" ? 8000 : 4500);
  }

  /* --- Montage d'un widget ----------------------------------------------- */

  /*
    Monte une instance de widget dans `frame` (élément .widget). Retourne un
    objet { refresh(), destroy() }.
  */
  function mountWidget(frame, instance, options = {}) {
    const definition = widgets.get(instance.widget);
    const body = frame.querySelector(".widget-body");
    const title = frame.querySelector(".widget-title");

    if (!definition) {
      title.textContent = "Widget indisponible";
      body.innerHTML = `<div class="widget-error">Le widget « ${escape(instance.widget)} » n’existe plus. Son module a peut-être été retiré.</div>`;
      return { refresh() {}, destroy() {} };
    }

    title.textContent = definition.title;
    title.title = definition.title;
    const head = frame.querySelector(".widget-head");
    head.querySelector(".widget-glyph")?.remove();
    head.insertAdjacentHTML("afterbegin",
      `<span class="widget-glyph">${icon(definition.icon || "grid", 13)}</span>`);
    let destroyed = false;
    let busy = false;

    const ctx = {
      instance,
      size: instance.size,
      settings: instance.settings || {},
      request: (method, params) => request(definition.module, method, params),
      open: (route, params) => request("core", "open", { page: routeString(route, params) }),
      tip,
      escape,
      icon,
      duration,
      durationHtml,
      ...options.context,
    };

    async function refresh() {
      if (destroyed || busy) return;
      busy = true;
      try {
        await definition.render(body, ctx);
      } catch (error) {
        body.innerHTML = `<div class="widget-error">${escape(error.message || error)}</div>`;
      } finally {
        busy = false;
      }
    }

    body.innerHTML = '<div class="skeleton" style="height:38px;width:60%"></div><div class="skeleton" style="height:12px;width:40%;margin-top:10px"></div>';
    refresh();
    const timer = definition.refresh > 0 ? setInterval(refresh, definition.refresh * 1000) : null;
    // Événements diffusés par l'hôte qui appellent un nouveau rendu.
    const unsubscribers = (definition.events || []).map((name) => on(name, refresh));

    return {
      refresh,
      destroy() {
        destroyed = true;
        if (timer) clearInterval(timer);
        unsubscribers.forEach((unsubscribe) => unsubscribe());
      },
    };
  }

  /* Une route : « module/page », avec paramètres facultatifs en « ?clé=valeur ». */
  function routeString(route, params) {
    if (!params) return route;
    const query = new URLSearchParams(params).toString();
    return query ? `${route}?${query}` : route;
  }

  /* --- Audit d'intégrité de la mise en page ------------------------------ */

  /*
    Mesure la page rendue et renvoie la liste des défauts visibles :
    défilement horizontal, élément qui sort de son conteneur ou que son cadre
    rogne, blocs voisins qui se chevauchent, texte coupé net ou tronqué sans
    infobulle, fenêtre flottante qui sort de l'écran.

    Utilisé par `scripts/audit/audit.py`, et appelable à la main depuis les
    outils de développement : `Atelier.audit()`.
  */
  function audit(root = document.body) {
    const TOLERANCE = 1;
    const issues = [];

    const describe = (element) => {
      const parts = [];
      for (let node = element; node && node !== document.body && parts.length < 4; node = node.parentElement) {
        let part = node.tagName.toLowerCase();
        if (node.id) part += `#${node.id}`;
        const classes = [...node.classList].slice(0, 2);
        if (classes.length) part += `.${classes.join(".")}`;
        parts.unshift(part);
      }
      const text = (element.textContent || "").trim().replace(/\s+/g, " ").slice(0, 60);
      return text ? `${parts.join(" > ")} « ${text} »` : parts.join(" > ");
    };

    const rectOf = (element) => {
      const r = element.getBoundingClientRect();
      return { x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) };
    };

    const add = (kind, element, detail, other) => {
      // Un seul signalement par élément et par nature : les descendants d'un
      // élément déjà signalé héritent du même défaut.
      for (let node = element.parentElement; node; node = node.parentElement) {
        if (node.auditFlag === kind) return;
      }
      element.auditFlag = kind;
      issues.push({
        kind,
        element: describe(element),
        rect: rectOf(element),
        detail,
        other: other ? describe(other) : undefined,
      });
    };

    const isVisible = (element) => {
      const style = getComputedStyle(element);
      if (style.display === "none" || style.visibility === "hidden" || Number(style.opacity) === 0) return false;
      const r = element.getBoundingClientRect();
      return r.width > 0.5 && r.height > 0.5;
    };

    // Les dessins SVG ont leur propre géométrie ; les infobulles et menus
    // flottent volontairement par-dessus le reste.
    const skip = (element) =>
      (element.closest("svg") && element.tagName.toLowerCase() !== "svg") ||
      element.closest(".tooltip:not(.visible)");

    const floating = (element) => ["absolute", "fixed"].includes(getComputedStyle(element).position);

    for (const node of document.querySelectorAll("*")) node.auditFlag = undefined;

    // 1. Défilement horizontal de la page.
    const page = document.scrollingElement || document.documentElement;
    if (page.scrollWidth > page.clientWidth + TOLERANCE) {
      issues.push({ kind: "défilement horizontal", element: "page", detail: `${page.scrollWidth} px de contenu pour ${page.clientWidth} px de fenêtre` });
    }

    const elements = [...root.querySelectorAll("*")].filter((element) => !skip(element) && isVisible(element));

    for (const element of elements) {
      const style = getComputedStyle(element);
      const box = element.getBoundingClientRect();
      const parent = element.parentElement;

      // 2. Sortie du conteneur direct (flux normal uniquement).
      const bleeds = style.getPropertyValue("--audit-bleed").trim() === "1";
      if (parent && parent !== document.documentElement && !floating(element) && style.display !== "contents" && !bleeds) {
        const outer = parent.getBoundingClientRect();
        const parentStyle = getComputedStyle(parent);
        const scrollsX = ["auto", "scroll"].includes(parentStyle.overflowX);
        const scrollsY = ["auto", "scroll"].includes(parentStyle.overflowY);
        const overX = !scrollsX && (box.left < outer.left - TOLERANCE || box.right > outer.right + TOLERANCE);
        const overY = !scrollsY && (box.top < outer.top - TOLERANCE || box.bottom > outer.bottom + TOLERANCE);
        if (overX || overY) {
          const dx = Math.max(outer.left - box.left, box.right - outer.right, 0);
          const dy = Math.max(outer.top - box.top, box.bottom - outer.bottom, 0);
          add("sort de son conteneur", element, `dépasse de ${Math.round(Math.max(dx, dy))} px`, parent);
        }
      }

      // 3. Contenu rogné par un ancêtre qui masque ce qui dépasse.
      for (let ancestor = parent; ancestor && ancestor !== document.body; ancestor = ancestor.parentElement) {
        const ancestorStyle = getComputedStyle(ancestor);
        const clipX = ["hidden", "clip"].includes(ancestorStyle.overflowX);
        const clipY = ["hidden", "clip"].includes(ancestorStyle.overflowY);
        if (!clipX && !clipY && ancestorStyle.overflowX === "visible" && ancestorStyle.overflowY === "visible") continue;
        const frame = ancestor.getBoundingClientRect();
        const cutX = clipX && (box.left < frame.left - TOLERANCE || box.right > frame.right + TOLERANCE);
        const cutY = clipY && (box.top < frame.top - TOLERANCE || box.bottom > frame.bottom + TOLERANCE);
        if (cutX || cutY) add("rogné par son cadre", element, "une partie n'est pas visible", ancestor);
        break;
      }

      // 4. Texte coupé net, ou tronqué sans le texte complet en infobulle.
      if (element.scrollWidth > element.clientWidth + TOLERANCE && style.overflowX !== "visible" && element.children.length === 0) {
        if (style.textOverflow === "ellipsis") {
          if (!element.closest("[title]")) add("tronqué sans infobulle", element, "le texte complet n'est lisible nulle part");
        } else if (!["auto", "scroll"].includes(style.overflowX)) {
          add("texte coupé", element, `${element.scrollWidth} px de texte pour ${element.clientWidth} px`);
        }
      }

      // 5. Blocs voisins qui se chevauchent.
      const children = [...element.children].filter((child) => !skip(child) && !floating(child) && isVisible(child));
      if (children.length >= 2 && children.length <= 120) {
        // Un élément en ligne qui passe à la ligne a un rectangle englobant
        // trompeur : on compare ses lignes une à une.
        const boxes = children.map((child) =>
          getComputedStyle(child).display === "inline" ? [...child.getClientRects()] : [child.getBoundingClientRect()]);
        for (let i = 0; i < children.length; i++) {
          for (let j = i + 1; j < children.length; j++) {
            let worst = null;
            for (const a of boxes[i]) {
              for (const b of boxes[j]) {
                const overlapX = Math.min(a.right, b.right) - Math.max(a.left, b.left);
                const overlapY = Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top);
                if (overlapX > TOLERANCE && overlapY > TOLERANCE) worst = [overlapX, overlapY];
              }
            }
            if (worst) {
              add("chevauchement", children[j], `recouvre ${Math.round(worst[0])} × ${Math.round(worst[1])} px`, children[i]);
            }
          }
        }
      }
    }

    // 6. Fenêtres flottantes ouvertes : entièrement à l'écran.
    for (const element of document.querySelectorAll(".popover, .modal, .toast, .tooltip.visible")) {
      if (!isVisible(element)) continue;
      const r = element.getBoundingClientRect();
      if (r.left < -TOLERANCE || r.top < -TOLERANCE || r.right > innerWidth + TOLERANCE || r.bottom > innerHeight + TOLERANCE) {
        add("hors de l'écran", element, `${Math.round(r.right)} × ${Math.round(r.bottom)} pour une fenêtre de ${innerWidth} × ${innerHeight}`);
      }
    }

    for (const node of document.querySelectorAll("*")) node.auditFlag = undefined;
    return issues;
  }

  function setTheme(dark) {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    emit("theme", dark);
  }

  return {
    boot,
    request,
    resolve,
    on,
    emit,
    module,
    widget,
    modules,
    widgets,
    failures,
    moduleFailed,
    escape,
    duration,
    durationHtml,
    clock,
    parseDate,
    isoDate,
    addDays,
    dateLabel,
    capitalize,
    delta,
    icon,
    sep,
    none,
    tip,
    showTooltip,
    hideTooltip,
    popover,
    closePopover,
    sizeLabel,
    sizeName,
    dialog,
    confirm,
    toast,
    mountWidget,
    audit,
    routeString,
    setTheme,
    NBSP,
    /* Remplacée par la fenêtre principale. */
    navigate() {},
  };
})();

window.Atelier = Atelier;
