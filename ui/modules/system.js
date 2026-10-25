/*
  Module « Système », moitié page : ce qui encombre l'ordinateur, et les
  actions pour y remédier. Les relevés viennent de la moitié Rust
  (`src/modules/system`).
*/

const MODULE = "system";
const T = Atelier;
const NB = T.NBSP;

/* --- Styles propres au module ------------------------------------------ */

const style = document.createElement("style");
style.textContent = `
/* Répartition de la mémoire : un segment par usage, le reste est libre. */
.sys-bar { display: flex; height: 12px; border-radius: 999px; overflow: hidden; background: var(--surface-3); }
.sys-bar.thin { height: 8px; }
.sys-seg { flex: 0 1 auto; min-width: 0; height: 100%; border-right: 2px solid var(--surface); }
.sys-seg:last-child { border-right: 0; }
.sys-seg.apps, .sys-swatch.apps { background: var(--accent); }
.sys-seg.kernel, .sys-swatch.kernel { background: var(--accent-strong); }
.sys-seg.kernel.soon, .sys-swatch.kernel.soon { background: var(--warning); }
.sys-seg.kernel.over, .sys-swatch.kernel.over { background: var(--critical); }
.sys-seg.other, .sys-swatch.other { background: color-mix(in srgb, var(--accent) 38%, var(--surface-3)); }
.sys-swatch.free { background: var(--surface-3); box-shadow: inset 0 0 0 1px var(--border-strong); }
.sys-swatch { width: 10px; height: 10px; border-radius: 3px; flex: none; }

.sys-legend { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 14px; margin-top: 14px; }
.sys-legend-key { display: flex; align-items: center; gap: 7px; min-width: 0; font-size: 12px; color: var(--text-2); }
.sys-legend-key span:last-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sys-legend-value { margin-top: 2px; font-family: var(--font-display); font-size: 17px; font-weight: 650; letter-spacing: -0.01em; font-variant-numeric: tabular-nums; }
.sys-legend-value.soon { color: var(--warning-text); }
.sys-legend-value.over { color: var(--critical-text); }

.sys-alerts { display: grid; gap: 10px; margin-top: 16px; }
.sys-alert { display: grid; grid-template-columns: 18px minmax(0, 1fr); gap: 10px; align-items: start; padding: 11px 12px; border-radius: var(--radius); background: var(--surface-2); }
.sys-alert .state { margin-top: 1px; }
.sys-alert-title { font-weight: 650; font-size: 12.5px; }
.sys-alert-detail, .sys-reason-detail { color: var(--text-2); font-size: 12px; }

.sys-stats { margin-top: 18px; }

/* Ce qui mérite l'attention. */
.sys-suspects { display: grid; gap: 10px; }
.sys-suspect { display: grid; grid-template-columns: 28px minmax(0, 1fr) auto; gap: 12px; align-items: start; padding: 12px 12px 13px; border: 1px solid var(--border); border-radius: var(--radius); }
.sys-suspect-main { min-width: 0; }
.sys-suspect-title { display: flex; align-items: center; gap: 8px; min-width: 0; font-weight: 650; }
.sys-suspect-title .name { flex: 0 1 auto; }
.sys-suspect-title .badge { flex: none; }
/* Le nom de l'exécutable se tronque ; numéro et durée restent entiers. */
.sys-meta { display: flex; align-items: center; min-width: 0; margin-top: 2px; color: var(--muted); font-size: 11.5px; white-space: nowrap; }
.sys-meta > span { flex: none; }
.sys-meta > .sys-meta-name { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
.sys-reasons { display: grid; gap: 8px; margin-top: 10px; }
.sys-actions { display: flex; align-items: center; gap: 6px; }

.sys-ignored { display: grid; gap: 2px; margin-top: 6px; }
.sys-ignored-row { display: grid; grid-template-columns: 22px minmax(0, 1fr) auto; gap: 10px; align-items: center; padding: 6px 0; border-top: 1px solid var(--border); }
.sys-ignored-row:first-child { border-top: 0; }

/* Tableau des processus. */
.sys-table { display: grid; }
.sys-row { display: grid; grid-template-columns: 14px 22px minmax(0, 1fr) 74px 86px 72px 92px 30px; gap: 10px; align-items: center; min-height: 40px; padding: 4px 8px; border-top: 1px solid var(--border); font-size: 12.5px; }
.sys-row.head { min-height: 30px; border-top: 0; color: var(--muted); font-size: 11.5px; font-weight: 650; }
.sys-row.head .n { color: var(--muted); }
.sys-row.clickable { cursor: pointer; }
.sys-row.clickable:hover { background: var(--surface-2); }
.sys-row.child { min-height: 34px; font-size: 12px; background: color-mix(in srgb, var(--surface-2) 45%, transparent); }
.sys-row .n { min-width: 0; overflow: hidden; color: var(--text-2); font-variant-numeric: tabular-nums; text-align: right; text-overflow: ellipsis; white-space: nowrap; }
.sys-row .n.strong { color: var(--text); font-weight: 600; }
.sys-row .n.alert { color: var(--critical-text); font-weight: 600; }
.sys-chevron { display: inline-grid; place-items: center; color: var(--muted); transition: transform 120ms ease; }
.sys-chevron.open { transform: rotate(90deg); }
.sys-cell-name { display: flex; align-items: center; gap: 8px; min-width: 0; }
.sys-cell-name .name { flex: 0 1 auto; }
.sys-cell-name .badge { flex: none; }
.sys-cell-sub { min-width: 0; overflow: hidden; color: var(--muted); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
.sys-row .btn-icon { width: 28px; height: 28px; }
@media (max-width: 1099px) {
  .sys-row { grid-template-columns: 14px 22px minmax(0, 1fr) 86px 72px 92px 30px; }
  .sys-row .opt { display: none; }
}

.sys-toolbar { display: flex; align-items: center; gap: 8px; }
.sys-toolbar .input { width: 200px; }
.sys-note { display: flex; align-items: center; gap: 10px; color: var(--text-2); font-size: 12.5px; }
.sys-note svg { color: var(--accent); }

/* Widgets. */
.sys-w-top { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
.sys-w-stats { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; margin-top: auto; padding-top: 10px; }
.sys-w-stats .stat-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; }
.sys-w-stats .stat-value { font-size: 15px; }
.sys-w-stats .stat-value.soon { color: var(--warning-text); }
.sys-w-stats .stat-value.over { color: var(--critical-text); }
.sys-w-text { min-width: 0; }
.sys-w-reason { overflow: hidden; font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
.sys-w-reason.over { color: var(--critical-text); }
.sys-w-reason.soon { color: var(--warning-text); }
.sys-w-reason.ok { color: var(--muted); }
.sys-mini { height: 24px; padding: 0 9px; border-radius: var(--radius-sm); font-size: 11.5px; }
.sys-done { color: var(--muted); font-size: 11.5px; white-space: nowrap; }
.sys-w-list .w-row { cursor: pointer; }
`;
document.head.appendChild(style);

/* --- Mise en forme ----------------------------------------------------- */

const decimal = new Intl.NumberFormat("fr-FR", { maximumFractionDigits: 1 });
const integer = new Intl.NumberFormat("fr-FR", { maximumFractionDigits: 0 });

function scaled(value) {
  const units = ["octets", "Ko", "Mo", "Go", "To"];
  let amount = Math.max(0, value || 0);
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit += 1;
  }
  const digits = unit >= 3 && amount < 100 ? 1 : 0;
  return { text: amount.toFixed(digits).replace(".", ","), unit: units[unit] };
}

/* « 7,3 Go », « 512 Mo ». */
function bytes(value) {
  const { text, unit } = scaled(value);
  return `${text}${NB}${unit}`;
}

/* Même chose, unité en retrait : pour les grands chiffres. */
function bytesHtml(value) {
  const { text, unit } = scaled(value);
  return `${text}<span class="unit">${unit}</span>`;
}

/* « 7,3 millions », « 52 000 ». */
function count(value) {
  if (value >= 1e6) {
    const millions = value / 1e6;
    return `${decimal.format(millions)}${NB}million${millions >= 2 ? "s" : ""}`;
  }
  return integer.format(value);
}

/* « 7,3 millions de handles », « 52 000 handles ». */
function countOf(value, noun) {
  return `${count(value)}${value >= 1e6 ? " de" : ""} ${noun}`;
}

function percent(value) {
  return `${value >= 10 ? integer.format(value) : decimal.format(value)}${NB}%`;
}

function since(milliseconds) {
  return milliseconds ? T.duration((Date.now() - milliseconds) / 1000) : "";
}

/* Message d'erreur de l'hôte, en phrase. */
function sentence(text) {
  const value = String(text || "").trim();
  if (!value) return "";
  return T.capitalize(value) + (/[.!?]$/.test(value) ? "" : ".");
}

function toneClass(tone) {
  return { critical: "over", warning: "soon" }[tone] || "ok";
}

function toneIcon(tone) {
  return tone === "info" ? "info" : "alert";
}

/* --- Noms et icônes des applications ----------------------------------- */

/*
  Ceux du module Temps d'écran, s'il est présent : ses clés sont les mêmes
  (chemin de l'exécutable en minuscules). À défaut, le nom de l'exécutable
  et une initiale.
*/
const catalog = { apps: {}, at: 0, loading: null };

async function ensureCatalog() {
  if (Date.now() - catalog.at < 60000) return;
  if (!catalog.loading) {
    catalog.loading = T.request("screen-time", "catalog")
      .then((result) => { catalog.apps = result.apps || {}; })
      .catch(() => { catalog.apps = {}; })
      .finally(() => { catalog.at = Date.now(); catalog.loading = null; });
  }
  await catalog.loading;
}

function displayName(item) {
  const info = catalog.apps[item.key];
  return (info && info.name) || item.name;
}

function appIcon(item, size = "") {
  const info = catalog.apps[item.key];
  const cls = `app-icon${size ? ` ${size}` : ""}`;
  if (info && info.icon) return `<img class="${cls}" src="${info.icon}" alt="" />`;
  return `<span class="${cls} placeholder">${T.escape(displayName(item).charAt(0).toUpperCase())}</span>`;
}

/* Nom tronqué si nécessaire, nom complet en infobulle. */
function nameSpan(item, extra = "") {
  const name = T.escape(displayName(item));
  return `<span class="name${extra ? ` ${extra}` : ""}" title="${name}">${name}</span>`;
}

/* --- Raisons et alertes ------------------------------------------------ */

function reasonInfo(reason, suspect) {
  switch (reason.kind) {
    case "handles":
      return {
        title: `${countOf(reason.value, "handles")} ouverts`,
        short: countOf(reason.value, "handles"),
        detail: "Un programme ordinaire en garde quelques milliers. Chacun occupe de la mémoire du noyau, rendue seulement à l’arrêt du processus.",
      };
    case "memory":
      return {
        title: `${bytes(reason.value)} de mémoire`,
        short: `${bytes(reason.value)} de mémoire`,
        detail: `${percent((reason.share || 0) * 100)} de la mémoire de l’ordinateur${suspect.targets.length > 1 ? `, répartis sur ${suspect.targets.length}${NB}processus` : ""}.`,
      };
    case "cpu":
      return {
        title: `${percent(reason.value)} du processeur`,
        short: `${percent(reason.value)} du processeur`,
        detail: `Sans relâche depuis plus de 30${NB}secondes, et sans aucune fenêtre ouverte.`,
      };
    default:
      return {
        title: "Resté seul",
        short: "resté seul",
        detail: "Une commande dont le terminal ou le script s’est fermé : plus personne ne lit ce qu’elle affiche.",
      };
  }
}

function alertInfo(alert, memory) {
  switch (alert.kind) {
    case "kernel":
      return {
        title: `Le noyau occupe ${bytes(alert.value)}`,
        detail: `${percent(alert.share * 100)} de la mémoire, contre 1 à 1,5${NB}Go d’ordinaire. La cause habituelle est un processus qui accumule les handles${NB}: ${count(memory.handles)} sont ouverts sur l’ordinateur.`,
      };
    case "pressure":
      return {
        title: `Il ne reste que ${bytes(alert.value)} de mémoire disponible`,
        detail: "Windows écrit sur le disque ce qui ne tient plus en mémoire : tout ralentit.",
      };
    default:
      return {
        title: "La mémoire engagée approche de sa limite",
        detail: `${bytes(alert.value)} sur ${bytes(memory.commitLimit)} possibles, fichier d’échange compris. Au-delà, des applications peuvent échouer.`,
      };
  }
}

/* --- Répartition de la mémoire ----------------------------------------- */

function memoryParts(breakdown, alerts) {
  const kernel = alerts.find((alert) => alert.kind === "kernel");
  const kernelAlert = kernel ? toneClass(kernel.tone) : "";
  return [
    { id: "apps", label: "Applications", value: breakdown.apps, tip: "Mémoire que chaque processus est seul à occuper, additionnée." },
    { id: "kernel", label: "Noyau", value: breakdown.kernel, alert: kernelAlert, tip: "Pilotes et objets du système : fichiers ouverts, handles, tampons." },
    { id: "other", label: "Partagé et système", value: breakdown.other, tip: "Bibliothèques communes à plusieurs programmes, fichiers projetés en mémoire, pilotes graphiques." },
    { id: "free", label: "Disponible", value: breakdown.available, tip: "Libre, ou occupé par un cache que Windows rend dès qu’un programme en a besoin." },
  ];
}

function memoryBar(memory, parts, thin = false) {
  const total = Math.max(1, memory.total);
  const used = parts.filter((part) => part.id !== "free");
  // Les arrondis ne doivent jamais faire dépasser la barre.
  const sum = used.reduce((value, part) => value + part.value, 0);
  const scale = sum > total ? total / sum : 1;
  const segments = used
    .filter((part) => part.value > 0)
    .map((part) => `<span class="sys-seg ${part.id}${part.alert ? ` ${part.alert}` : ""}" data-part="${part.id}" style="width:${(part.value * scale / total) * 100}%"></span>`)
    .join("");
  return `<div class="sys-bar${thin ? " thin" : ""}" role="img" aria-label="Répartition de la mémoire">${segments}</div>`;
}

function attachPartTips(root, memory, parts) {
  root.querySelectorAll("[data-part]").forEach((node) => {
    const part = parts.find((candidate) => candidate.id === node.dataset.part);
    T.tip(node, `<div class="tooltip-title">${T.escape(part.label)}</div>
      <div class="tooltip-row"><span class="grow">${bytes(part.value)}</span><b>${percent(part.value / Math.max(1, memory.total) * 100)}</b></div>
      <div class="tooltip-row" style="white-space:normal">${T.escape(part.tip)}</div>`);
  });
}

/* --- Actions ----------------------------------------------------------- */

/* Pendant une boîte de dialogue ou un menu, la page ne se redessine pas. */
function interacting() {
  return Boolean(document.querySelector(".backdrop, .popover"));
}

async function stopTargets(request, item, targets) {
  try {
    const result = await request("terminate", { targets });
    const name = displayName(item);
    T.toast(result.stopped > 1 ? `${result.stopped}${NB}processus de ${name} arrêtés.` : `${name} est arrêté.`, "success");
    if (result.errors.length) T.toast(sentence(result.errors[0]), "error");
    return true;
  } catch (error) {
    T.toast(sentence(error.message), "error");
    return false;
  }
}

async function closeWindows(request, item, entries) {
  let windows = 0;
  let failure = null;
  for (const entry of entries) {
    try {
      const result = await request("close", { target: { pid: entry.pid, started: entry.started } });
      windows += result.windows;
    } catch (error) {
      failure = error;
    }
  }
  if (windows) {
    T.toast(`${displayName(item)} a reçu la demande de fermeture. Elle peut vous proposer d’enregistrer.`, "success");
  } else if (failure) {
    T.toast(sentence(failure.message), "error");
  }
}

/*
  Confirme puis arrête. Une application qui a des fenêtres se voit d'abord
  proposer une fermeture polie, qui lui laisse le temps d'enregistrer.
*/
function confirmStop(request, item, options, done) {
  const targets = options.targets;
  const many = targets.length > 1;
  const name = displayName(item);
  const windowed = (options.windowed || []).length > 0;

  T.dialog({
    title: many ? `Arrêter les ${targets.length}${NB}processus de ${name}${NB}?` : `Arrêter ${name}${NB}?`,
    build(body) {
      body.innerHTML = `
        <div style="color:var(--text-2)">
          <p style="margin:0 0 8px">L’arrêt est immédiat${NB}: ${many ? "ces processus n’ont" : "le processus n’a"} pas l’occasion d’enregistrer, et ce qui ne l’était pas est perdu.</p>
          ${windowed ? "<p style=\"margin:0\">Pour laisser à l’application le temps d’enregistrer, demandez plutôt la fermeture de ses fenêtres.</p>" : ""}
        </div>
        ${item.path ? `<div class="path" title="${T.escape(item.path)}">${T.escape(item.path)}</div>` : ""}`;
    },
    actions: [
      { label: "Annuler", ghost: true },
      { spacer: true },
      ...(windowed ? [{
        label: "Demander la fermeture",
        run: async (close) => { close(); await closeWindows(request, item, options.windowed); done(); },
      }] : []),
      {
        label: "Arrêter",
        danger: true,
        icon: "power",
        run: async (close) => { close(); await stopTargets(request, item, targets); done(); },
      },
    ],
  });
}

async function setIgnored(request, item, ignored) {
  try {
    await request("ignore", { key: item.key, ignored });
    T.toast(ignored
      ? `${displayName(item)} n’est plus signalée. Elle reste dans la liste des processus.`
      : `${displayName(item)} est de nouveau surveillée.`, "success");
  } catch (error) {
    T.toast(sentence(error.message), "error");
  }
}

async function reveal(request, item) {
  try {
    await request("reveal", { key: item.key });
  } catch (error) {
    T.toast(sentence(error.message), "error");
  }
}

function pageHead(title, lede, actions = "") {
  return `
    <div class="page-head">
      <div class="titles">
        <h1 class="page-title">${title}</h1>
        ${lede ? `<p class="page-lede">${lede}</p>` : ""}
      </div>
      <div class="page-actions">${actions}</div>
    </div>`;
}

function demoNote(demo) {
  return demo
    ? `<section class="card"><div class="sys-note">${T.icon("info", 16)}<span>Mode démonstration${NB}: le relevé est fictif, et aucune action ne vise un vrai processus.</span></div></section>`
    : "";
}

/* --- Page : l'encombrement --------------------------------------------- */

function suspectMeta(suspect) {
  const full = `${suspect.name}${suspect.path ? ` (${suspect.path})` : ""}`;
  // Le nom de l'exécutable, s'il n'est pas déjà le titre de la carte.
  const parts = displayName(suspect) === suspect.name
    ? []
    : [`<span class="sys-meta-name" title="${T.escape(full)}">${T.escape(suspect.name)}</span>`];
  if (suspect.targets.length === 1) parts.push(`<span>PID ${suspect.targets[0].pid}</span>`);
  else if (suspect.targets.length > 1) parts.push(`<span>${suspect.targets.length}${NB}processus</span>`);
  if (suspect.started) parts.push(`<span class="sys-meta-name" title="${T.escape(full)}">lancé il y a ${since(suspect.started)}</span>`);
  return parts.join(T.sep);
}

function suspectCard(suspect) {
  const reasons = suspect.reasons.map((reason) => {
    const info = reasonInfo(reason, suspect);
    return `<div>
      <span class="state ${toneClass(reason.tone)}">${T.icon(toneIcon(reason.tone), 13)}${T.escape(info.title)}</span>
      <div class="sys-reason-detail">${T.escape(info.detail)}</div>
    </div>`;
  }).join("");

  return `
    <div class="sys-suspect" data-id="${T.escape(suspect.id)}">
      ${appIcon(suspect, "md")}
      <div class="sys-suspect-main">
        <div class="sys-suspect-title">${nameSpan(suspect)}${suspect.protected ? '<span class="badge">protégé</span>' : ""}</div>
        <div class="sys-meta">${suspectMeta(suspect)}</div>
        <div class="sys-reasons">${reasons}</div>
      </div>
      <div class="sys-actions">
        ${suspect.protected || !suspect.targets.length ? "" : `<button class="btn btn-danger" type="button" data-stop>${T.icon("power", 15)}Arrêter</button>`}
        <button class="btn-icon" type="button" data-menu aria-label="Autres actions" aria-expanded="false">${T.icon("more", 16)}</button>
      </div>
    </div>`;
}

async function renderOverview(root, ctx) {
  async function draw() {
    if (interacting()) return;
    const data = await ctx.request("overview");
    await ensureCatalog();
    const { memory, breakdown, alerts, suspects } = data;
    const parts = memoryParts(breakdown, alerts);
    const usedShare = breakdown.used / Math.max(1, memory.total) * 100;

    root.innerHTML = pageHead("Encombrement",
      "Ce qui occupe la mémoire de l’ordinateur, et les processus qui ont l’air anormaux.",
      `<button class="btn" id="all" type="button">${T.icon("apps", 15)}Tous les processus</button>`) + `
      <div class="stack">
        ${demoNote(data.demo)}
        <section class="card">
          <div class="card-head"><div class="card-title">Mémoire</div><div class="spacer"></div><div class="card-sub">${bytes(memory.total)} au total</div></div>
          <div style="display:flex;align-items:baseline;gap:12px;margin-bottom:14px">
            <div class="hero" style="font-size:44px">${bytesHtml(breakdown.used)}</div>
            <div class="stat-note" style="margin:0">utilisés, soit ${percent(usedShare)}</div>
          </div>
          ${memoryBar(memory, parts)}
          <div class="sys-legend">${parts.map((part) => `
            <div style="min-width:0">
              <div class="sys-legend-key" title="${T.escape(part.label)}"><span class="sys-swatch ${part.id}${part.alert ? ` ${part.alert}` : ""}"></span><span>${T.escape(part.label)}</span></div>
              <div class="sys-legend-value${part.alert ? ` ${part.alert}` : ""}">${bytes(part.value)}</div>
            </div>`).join("")}
          </div>
          ${alerts.length ? `<div class="sys-alerts">${alerts.map((alert) => {
            const info = alertInfo(alert, memory);
            return `<div class="sys-alert">
              <span class="state ${toneClass(alert.tone)}">${T.icon("alert", 15)}</span>
              <div style="min-width:0"><div class="sys-alert-title">${T.escape(info.title)}</div><div class="sys-alert-detail">${T.escape(info.detail)}</div></div>
            </div>`;
          }).join("")}</div>` : ""}
          <div class="grid-3 sys-stats">
            <div>
              <div class="stat-label">Mémoire engagée</div>
              <div class="stat-value">${bytesHtml(memory.commit)}</div>
              <div class="stat-note">sur ${bytes(memory.commitLimit)} possibles, fichier d’échange compris</div>
            </div>
            <div>
              <div class="stat-label">Handles ouverts</div>
              <div class="stat-value">${T.escape(count(memory.handles))}</div>
              <div class="stat-note">sur tout l’ordinateur</div>
            </div>
            <div>
              <div class="stat-label">Processus</div>
              <div class="stat-value">${integer.format(memory.processes)}</div>
              <div class="stat-note">${integer.format(memory.threads)}${NB}fils d’exécution</div>
            </div>
          </div>
        </section>

        <section class="card">
          <div class="card-head">
            <div class="card-title">À traiter</div>
            <div class="spacer"></div>
            <div class="card-sub">${suspects.length ? `${suspects.length}${NB}élément${suspects.length > 1 ? "s" : ""}` : "Vérifié toutes les 10 secondes"}</div>
          </div>
          ${suspects.length
            ? `<div class="sys-suspects">${suspects.map(suspectCard).join("")}</div>`
            : `<div class="empty">${T.icon("check", 26)}Rien d’anormal pour l’instant.</div>`}
        </section>

        <section class="card">
          <div class="card-head"><div class="card-title">Les plus gourmandes</div><div class="spacer"></div><div class="card-sub">Mémoire propre à chaque application</div></div>
          <div id="top"></div>
        </section>

        <section class="card">
          <div class="card-head"><div class="card-title">Surveillance</div></div>
          <label class="toggle">
            <input type="checkbox" id="notify" ${data.notify ? "checked" : ""} />
            <span class="track"><span class="thumb"></span></span>
            <span class="toggle-text">
              <span class="toggle-title">Prévenir quand un processus devient anormal</span>
              <span class="toggle-desc">Une notification propose de l’arrêter quand un processus accumule les handles, ou accapare la mémoire sans aucune fenêtre ouverte.</span>
            </span>
          </label>
          ${data.ignored.length ? `
            <div class="field-label" style="margin-top:14px">Applications ignorées</div>
            <div class="sys-ignored">${data.ignored.map((key) => {
              const item = { key, name: key.split("\\").pop() };
              return `<div class="sys-ignored-row">${appIcon(item)}<div style="min-width:0">${nameSpan(item, "block medium")}<div class="sys-cell-sub" title="${T.escape(key)}">${T.escape(key)}</div></div>
                <button class="btn" type="button" data-unignore="${T.escape(key)}">Signaler de nouveau</button></div>`;
            }).join("")}</div>` : ""}
        </section>
      </div>`;

    attachPartTips(root, memory, parts);
    root.querySelector("#all").addEventListener("click", () => ctx.navigate("processes"));
    root.querySelector("#notify").addEventListener("change", async (event) => {
      try {
        await ctx.request("settings.save", { notify: event.target.checked });
      } catch (error) {
        T.toast(sentence(error.message), "error");
      }
    });
    root.querySelectorAll("[data-unignore]").forEach((button) => button.addEventListener("click", async () => {
      await setIgnored(ctx.request, { key: button.dataset.unignore, name: button.dataset.unignore.split("\\").pop() }, false);
      draw();
    }));

    const top = data.top.filter((group) => group.privateBytes > 0);
    const max = Math.max(1, ...top.map((group) => group.privateBytes));
    root.querySelector("#top").innerHTML = top.length ? `<div class="rank">${top.map((group, index) => `
      <div class="rank-row clickable" data-index="${index}" title="${T.escape(group.path || group.name)}">
        ${appIcon(group)}
        <div class="rank-name">${T.escape(displayName(group))}<small>${T.escape(group.name)}${group.count > 1 ? ` (${group.count}${NB}processus)` : ""}</small></div>
        <div class="rank-track"><div class="rank-bar" style="width:${(group.privateBytes / max) * 100}%"></div></div>
        <div class="rank-value">${bytes(group.privateBytes)}</div>
      </div>`).join("")}</div>` : `<div class="empty">${T.icon("apps", 26)}Aucune mesure disponible.</div>`;
    root.querySelectorAll("#top .rank-row").forEach((row) => row.addEventListener("click", () =>
      ctx.navigate("processes", { focus: top[Number(row.dataset.index)].key })));

    root.querySelectorAll(".sys-suspect").forEach((card) => {
      const suspect = suspects.find((candidate) => candidate.id === card.dataset.id);
      const stop = card.querySelector("[data-stop]");
      if (stop) {
        stop.addEventListener("click", () => confirmStop(ctx.request, suspect, {
          targets: suspect.targets,
          windowed: suspect.windowed && suspect.targets.length === 1 ? suspect.targets : [],
        }, draw));
      }
      const menu = card.querySelector("[data-menu]");
      menu.addEventListener("click", () => {
        const items = [];
        if (suspect.protected) items.push({ label: "Processus de Windows ou d’Atelier", heading: true });
        if (!suspect.protected && suspect.windowed && suspect.targets.length === 1) {
          items.push({ label: "Demander la fermeture", icon: "x", action: async () => { await closeWindows(ctx.request, suspect, suspect.targets); draw(); } });
        }
        if (suspect.path) items.push({ label: "Ouvrir l’emplacement", icon: "folder", action: () => reveal(ctx.request, suspect) });
        items.push({ label: "Voir dans les processus", icon: "apps", action: () => ctx.navigate("processes", { focus: suspect.key }) });
        items.push("separator");
        items.push({ label: "Ne plus signaler cette application", icon: "eyeOff", action: async () => { await setIgnored(ctx.request, suspect, true); draw(); } });
        T.popover(menu, items);
      });
    });
  }

  await draw();
  ctx.every(4, () => draw().catch((error) => console.error(error)));
}

/* --- Page : les processus ---------------------------------------------- */

const SORTS = [
  { id: "memory", label: "Mémoire", value: (group) => group.privateBytes },
  { id: "cpu", label: "Processeur", value: (group) => group.cpu },
  { id: "handles", label: "Handles", value: (group) => group.handles },
  { id: "name", label: "Nom" },
];

async function renderProcesses(root, ctx) {
  let sort = "memory";
  let query = "";
  let data = null;
  const expanded = new Set(ctx.params.focus ? [ctx.params.focus] : []);
  let focusPending = Boolean(ctx.params.focus);

  root.innerHTML = pageHead("Processus",
    "Chaque application et la somme de ses processus.",
    `<div class="sys-toolbar">
      <input class="input" id="search" placeholder="Rechercher…" />
      <div class="segmented" id="sort">${SORTS.map((option) => `<button type="button" data-sort="${option.id}" aria-pressed="${option.id === sort}">${option.label}</button>`).join("")}</div>
    </div>`) + `
    <div class="stack">
      <div id="demo"></div>
      <section class="card" style="padding:8px">
        <div id="table"></div>
      </section>
    </div>`;

  const handlesAlert = (value) => value >= 50000;

  function processMenu(anchor, group, entry) {
    const target = { pid: entry.pid, started: entry.started };
    const items = [{ label: `PID ${entry.pid}`, heading: true }];
    if (entry.protected) {
      items.push({ label: "Processus de Windows ou d’Atelier", heading: true });
    } else {
      if (entry.windowed) {
        items.push({ label: "Demander la fermeture", icon: "x", action: async () => { await closeWindows(ctx.request, group, [target]); refresh(); } });
      }
      items.push({
        label: "Arrêter ce processus",
        icon: "power",
        danger: true,
        action: () => confirmStop(ctx.request, group, { targets: [target], windowed: entry.windowed ? [target] : [] }, refresh),
      });
    }
    T.popover(anchor, items);
  }

  function groupMenu(anchor, group) {
    const stoppable = group.processes.filter((entry) => !entry.protected);
    const windowed = stoppable.filter((entry) => entry.windowed).map((entry) => ({ pid: entry.pid, started: entry.started }));
    const targets = stoppable.map((entry) => ({ pid: entry.pid, started: entry.started }));
    const ignored = data.ignored.includes(group.key);
    const items = [];
    if (group.protected) items.push({ label: "Processus de Windows ou d’Atelier", heading: true });
    if (windowed.length) {
      items.push({ label: "Demander la fermeture", icon: "x", action: async () => { await closeWindows(ctx.request, group, windowed); refresh(); } });
    }
    if (targets.length) {
      items.push({
        label: targets.length > 1 ? `Arrêter les ${targets.length}${NB}processus` : "Arrêter",
        icon: "power",
        danger: true,
        action: () => confirmStop(ctx.request, group, { targets, windowed }, refresh),
      });
    }
    if (group.path) items.push({ label: "Ouvrir l’emplacement", icon: "folder", action: () => reveal(ctx.request, group) });
    items.push("separator");
    items.push(ignored
      ? { label: "Signaler de nouveau cette application", icon: "eye", action: async () => { await setIgnored(ctx.request, group, false); refresh(); } }
      : { label: "Ne plus signaler cette application", icon: "eyeOff", action: async () => { await setIgnored(ctx.request, group, true); refresh(); } });
    T.popover(anchor, items);
  }

  function draw() {
    const table = root.querySelector("#table");
    root.querySelector("#demo").innerHTML = demoNote(data.demo);

    const option = SORTS.find((candidate) => candidate.id === sort);
    const groups = data.groups
      .filter((group) => !query || [displayName(group), group.name, group.path || ""].some((text) => text.toLowerCase().includes(query)))
      .sort((a, b) => sort === "name"
        ? displayName(a).localeCompare(displayName(b), "fr")
        : option.value(b) - option.value(a));

    if (!groups.length) {
      table.innerHTML = `<div class="empty">${T.icon("search", 26)}Aucun processus ne correspond.</div>`;
      return;
    }

    const head = `<div class="sys-row head">
      <span></span><span></span><span>Application</span><span class="n opt">Processus</span>
      <span class="n">Mémoire</span><span class="n">Processeur</span><span class="n">Handles</span><span></span></div>`;

    const rows = groups.map((group) => {
      const open = expanded.has(group.key);
      const multiple = group.count > 1;
      const sub = `${group.name}${group.path ? ` (${group.path})` : ""}`;
      let html = `
        <div class="sys-row${multiple ? " clickable" : ""}" data-key="${T.escape(group.key)}">
          <span class="sys-chevron${open ? " open" : ""}">${multiple ? T.icon("chevronRight", 13) : ""}</span>
          ${appIcon(group)}
          <div style="min-width:0">
            <div class="sys-cell-name">${nameSpan(group, "medium")}${data.ignored.includes(group.key) ? '<span class="badge mini">ignorée</span>' : ""}</div>
            <div class="sys-cell-sub" title="${T.escape(sub)}">${T.escape(group.name)}</div>
          </div>
          <span class="n opt">${group.count}</span>
          <span class="n strong">${group.privateBytes ? bytes(group.privateBytes) : T.none}</span>
          <span class="n">${percent(group.cpu)}</span>
          <span class="n${handlesAlert(group.handles) ? " alert" : ""}" title="${integer.format(group.handles)}">${count(group.handles)}</span>
          <button class="btn-icon" type="button" data-group-menu aria-label="Actions" aria-expanded="false">${T.icon("more", 16)}</button>
        </div>`;

      // Une application d'un seul processus n'a pas de détail à ouvrir :
      // ses actions passent par le menu de sa ligne.
      if (multiple && open) {
        html += group.processes.map((entry) => `
            <div class="sys-row child" data-key="${T.escape(group.key)}" data-pid="${entry.pid}">
              <span></span><span></span>
              <div class="sys-cell-name"><span class="name" title="PID ${entry.pid}">PID ${entry.pid}${entry.started ? `${T.sep}depuis ${since(entry.started)}` : ""}</span>${entry.orphan ? '<span class="badge mini">resté seul</span>' : ""}${entry.windowed ? '<span class="badge mini">fenêtre</span>' : ""}</div>
              <span class="n opt">${entry.threads}${NB}fils</span>
              <span class="n">${entry.privateBytes ? bytes(entry.privateBytes) : T.none}</span>
              <span class="n">${percent(entry.cpu)}</span>
              <span class="n${handlesAlert(entry.handles) ? " alert" : ""}" title="${integer.format(entry.handles)}">${count(entry.handles)}</span>
              <button class="btn-icon" type="button" data-process-menu aria-label="Actions sur ce processus" aria-expanded="false">${T.icon("more", 16)}</button>
            </div>`).join("");
      }
      return html;
    }).join("");

    table.innerHTML = `<div class="sys-table">${head}${rows}</div>`;

    table.querySelectorAll(".sys-row[data-key]:not(.child)").forEach((row) => {
      const group = groups.find((candidate) => candidate.key === row.dataset.key);
      row.querySelector("[data-group-menu]").addEventListener("click", (event) => {
        event.stopPropagation();
        groupMenu(event.currentTarget, group);
      });
      if (group.count > 1) {
        row.addEventListener("click", () => {
          if (expanded.has(group.key)) expanded.delete(group.key);
          else expanded.add(group.key);
          draw();
        });
      }
    });
    table.querySelectorAll(".sys-row.child").forEach((row) => {
      const group = groups.find((candidate) => candidate.key === row.dataset.key);
      const entry = group.processes.find((candidate) => String(candidate.pid) === row.dataset.pid);
      row.querySelector("[data-process-menu]").addEventListener("click", (event) => processMenu(event.currentTarget, group, entry));
    });

    if (focusPending) {
      focusPending = false;
      const focused = [...table.querySelectorAll(".sys-row[data-key]:not(.child)")].find((row) => row.dataset.key === ctx.params.focus);
      if (focused) focused.scrollIntoView({ block: "center" });
    }
  }

  async function refresh() {
    if (interacting()) return;
    data = await ctx.request("processes");
    await ensureCatalog();
    draw();
  }

  root.querySelector("#search").addEventListener("input", (event) => {
    query = event.target.value.trim().toLowerCase();
    if (data) draw();
  });
  root.querySelectorAll("#sort button").forEach((button) => button.addEventListener("click", () => {
    sort = button.dataset.sort;
    root.querySelectorAll("#sort button").forEach((other) => other.setAttribute("aria-pressed", String(other === button)));
    if (data) draw();
  }));

  await refresh();
  ctx.every(4, () => refresh().catch((error) => console.error(error)));
}

/* --- Déclaration du module --------------------------------------------- */

T.module({
  id: MODULE,
  name: "Système",
  icon: "gauge",
  description: "Ce qui encombre la mémoire et le processeur, et de quoi y remédier.",
  pages: [
    { id: "overview", title: "Encombrement", icon: "gauge", render: renderOverview },
    { id: "processes", title: "Processus", icon: "cpu", render: renderProcesses },
  ],
});

/* --- Widgets ----------------------------------------------------------- */

/*
  Remplit un widget avec autant de lignes que sa hauteur en accueille, puis
  « et N autres » (sauf `more: false`). Le nombre de lignes est mesuré,
  jamais fixé à l'avance.
*/
function fitRows(el, rows, renderRow, options = {}) {
  el.innerHTML = `<div class="w-list sys-w-list"${options.gap ? ` style="gap:${options.gap}px"` : ""}></div><div class="w-foot"></div>`;
  const list = el.firstElementChild;
  const foot = el.lastElementChild;
  const overflowing = () => el.scrollHeight > el.clientHeight + 0.5;
  const more = options.more !== false;
  const setFoot = (shown) => {
    const rest = more ? rows.length - shown : 0;
    foot.hidden = rest <= 0;
    foot.textContent = rest > 0 ? `et ${rest}${NB}autre${rest > 1 ? "s" : ""}` : "";
  };

  let shown = 0;
  for (const row of rows) {
    list.insertAdjacentHTML("beforeend", renderRow(row));
    setFoot(shown + 1);
    if (overflowing()) {
      list.lastElementChild.remove();
      break;
    }
    shown += 1;
  }
  setFoot(shown);
  while (shown > 0 && overflowing()) {
    list.lastElementChild.remove();
    shown -= 1;
    setFoot(shown);
  }
  return rows.slice(0, shown);
}

T.widget({
  id: "system.memory",
  module: MODULE,
  title: "Mémoire",
  description: "Ce qui occupe la mémoire : applications, noyau, et ce qui reste disponible.",
  icon: "memory",
  sizes: ["m", "s"],
  refresh: 4,
  async render(el, ctx) {
    const data = await ctx.request("overview");
    const { memory, breakdown, alerts } = data;
    const parts = memoryParts(breakdown, alerts);
    const share = breakdown.used / Math.max(1, memory.total) * 100;
    const kernelAlert = parts.find((part) => part.id === "kernel").alert;

    if (ctx.size === "s") {
      el.innerHTML = `
        <div class="w-hero" style="font-size:36px">${Math.round(share)}<span class="unit">%</span></div>
        <div class="w-sub">${bytes(breakdown.available)} disponibles</div>
        <div class="w-foot" style="padding-top:10px">${memoryBar(memory, parts, true)}</div>`;
      attachPartTips(el, memory, parts);
      return;
    }

    el.innerHTML = `
      <div class="sys-w-top">
        <div class="w-hero" style="font-size:30px">${bytesHtml(breakdown.used)}</div>
        <div class="w-sub" style="margin:0">sur ${bytes(memory.total)}, soit ${percent(share)}</div>
      </div>
      <div style="margin-top:10px">${memoryBar(memory, parts)}</div>
      <div class="sys-w-stats">
        <div style="min-width:0"><div class="stat-label" title="Applications">Applications</div><div class="stat-value">${bytesHtml(breakdown.apps)}</div></div>
        <div style="min-width:0"><div class="stat-label" title="Noyau">Noyau</div><div class="stat-value${kernelAlert ? ` ${kernelAlert}` : ""}">${bytesHtml(breakdown.kernel)}</div></div>
        <div style="min-width:0"><div class="stat-label" title="Disponible">Disponible</div><div class="stat-value">${bytesHtml(breakdown.available)}</div></div>
      </div>`;
    attachPartTips(el, memory, parts);
    el.querySelector(".sys-bar").addEventListener("click", () => ctx.open("system/overview"));
  },
});

/*
  Arrêt en deux temps dans un widget, trop petit pour une boîte de dialogue :
  le premier clic arme le bouton, le second, dans les 4 secondes, arrête.
*/
const armed = new Map();
const outcomes = new Map();

T.widget({
  id: "system.clutter",
  module: MODULE,
  title: "Ce qui encombre",
  description: "Les processus anormaux, avec de quoi les arrêter sur place.",
  icon: "gauge",
  sizes: ["m", "l"],
  refresh: 5,
  async render(el, ctx) {
    const data = await ctx.request("overview");
    await ensureCatalog();
    const now = Date.now();
    for (const [id, until] of armed) if (until < now) armed.delete(id);
    for (const [id, outcome] of outcomes) if (outcome.until < now) outcomes.delete(id);

    const suspects = data.suspects;
    if (!suspects.length) {
      const top = data.top[0];
      el.innerHTML = `<div class="empty" style="padding:10px 0">${T.icon("check", 24)}Rien d’anormal
        ${top ? `<span class="name block" title="${T.escape(displayName(top))}" style="max-width:100%;min-width:0">La plus gourmande${NB}: ${T.escape(displayName(top))}, ${bytes(top.privateBytes)}</span>` : ""}</div>`;
      return;
    }

    const shown = fitRows(el, suspects, (suspect) => {
      const reason = suspect.reasons[0];
      const info = reasonInfo(reason, suspect);
      const outcome = outcomes.get(suspect.id);
      let action = "";
      if (outcome) {
        action = `<span class="sys-done" title="${T.escape(outcome.text)}">${T.escape(outcome.short)}</span>`;
      } else if (!suspect.protected && suspect.targets.length) {
        action = armed.has(suspect.id)
          ? `<button class="btn btn-danger sys-mini" type="button" data-stop="${T.escape(suspect.id)}">Confirmer</button>`
          : `<button class="btn sys-mini" type="button" data-stop="${T.escape(suspect.id)}">Arrêter</button>`;
      }
      return `<div class="w-row" data-id="${T.escape(suspect.id)}">${appIcon(suspect)}
        <div class="sys-w-text">${nameSpan(suspect, "block")}<div class="sys-w-reason ${toneClass(reason.tone)}" title="${T.escape(info.title)}">${T.escape(info.short)}</div></div>
        ${action || "<span></span>"}</div>`;
    }, { gap: 9 });

    el.querySelectorAll(".w-row[data-id]").forEach((row) => {
      row.addEventListener("click", () => ctx.open("system/overview"));
    });
    el.querySelectorAll("[data-stop]").forEach((button) => button.addEventListener("click", async (event) => {
      event.stopPropagation();
      const suspect = shown.find((candidate) => candidate.id === button.dataset.stop);
      if (!armed.has(suspect.id)) {
        armed.set(suspect.id, Date.now() + 4000);
        this.render(el, ctx);
        setTimeout(() => { if (armed.delete(suspect.id)) this.render(el, ctx); }, 4100);
        return;
      }
      armed.delete(suspect.id);
      try {
        const result = await ctx.request("terminate", { targets: suspect.targets });
        outcomes.set(suspect.id, { short: "Arrêté", text: `${result.stopped}${NB}processus arrêté${result.stopped > 1 ? "s" : ""}`, until: Date.now() + 6000 });
      } catch (error) {
        outcomes.set(suspect.id, { short: "Échec", text: sentence(error.message), until: Date.now() + 8000 });
      }
      this.render(el, ctx);
    }));
  },
});

T.widget({
  id: "system.top",
  module: MODULE,
  title: "Les plus gourmandes",
  description: "Les applications qui occupent le plus de mémoire, tous processus confondus.",
  icon: "memory",
  sizes: ["m", "l"],
  refresh: 5,
  async render(el, ctx) {
    const data = await ctx.request("overview");
    await ensureCatalog();
    const top = data.top.filter((group) => group.privateBytes > 0);
    if (!top.length) {
      el.innerHTML = `<div class="empty">${T.icon("memory", 24)}Aucune mesure disponible</div>`;
      return;
    }
    const max = Math.max(1, ...top.map((group) => group.privateBytes));
    fitRows(el, top, (group) => `
      <div class="w-row" title="${T.escape(group.path || group.name)}">${appIcon(group)}${nameSpan(group)}<span class="value">${bytes(group.privateBytes)}</span>
      <span></span><div class="bar" style="width:${(group.privateBytes / max) * 100}%"></div></div>`,
    // Un classement s'arrête à ce qui tient, sans « et N autres ».
    { gap: ctx.size === "l" ? 11 : 8, more: false });
    el.querySelectorAll(".w-row").forEach((row) => row.addEventListener("click", () => ctx.open("system/processes")));
  },
});
