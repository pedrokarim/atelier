/*
  Module « Temps d'écran », moitié page : ses pages, ses graphiques et ses
  widgets. Les données viennent de la moitié Rust (`src/modules/screen_time`).
*/

const MODULE = "screen-time";
const T = Atelier;
const NB = T.NBSP;

/* --- Catalogue des applications (noms et icônes) ------------------------ */

const catalog = { version: -1, apps: {}, loading: null };

async function ensureCatalog(request, version) {
  if (version === catalog.version) return;
  if (!catalog.loading) {
    catalog.loading = request("catalog").then((result) => {
      catalog.version = result.version;
      catalog.apps = result.apps;
    }).finally(() => { catalog.loading = null; });
  }
  await catalog.loading;
}

function appName(key) {
  const info = catalog.apps[key];
  if (info) return info.name;
  const file = key.split("\\").pop().replace(/\.exe$/, "");
  return file.charAt(0).toUpperCase() + file.slice(1);
}

function appIcon(key, size = "") {
  const info = catalog.apps[key];
  const cls = `app-icon${size ? ` ${size}` : ""}`;
  if (info && info.icon) return `<img class="${cls}" src="${info.icon}" alt="" />`;
  return `<span class="${cls} placeholder">${T.escape(appName(key).charAt(0).toUpperCase())}</span>`;
}

/* --- Graphiques ---------------------------------------------------------- */

/* Rectangle à extrémité arrondie côté données, carré côté ligne de base. */
function columnPath(x, y, width, height, radius = 4) {
  if (height <= 0) return "";
  const r = Math.min(radius, width / 2, height);
  return `M${x},${y + height}V${y + r}Q${x},${y} ${x + r},${y}H${x + width - r}Q${x + width},${y} ${x + width},${y + r}V${y + height}Z`;
}

function niceMax(value) {
  // Graduations rondes en minutes ou en heures.
  const steps = [300, 600, 900, 1800, 3600, 7200, 10800, 14400, 21600, 28800, 43200, 86400];
  for (const step of steps) if (value <= step) return step;
  return Math.ceil(value / 3600) * 3600;
}

function tickLabel(seconds) {
  if (seconds === 0) return "0";
  if (seconds < 3600) return `${Math.round(seconds / 60)}${NB}min`;
  return `${+(seconds / 3600).toFixed(1)}${NB}h`;
}

/*
  Histogramme en colonnes. `bars` : [{ value, label, tip, onClick, dim }].
  Une seule série, une seule couleur : le titre de la carte dit ce qui est
  tracé ; les valeurs exactes passent par l'infobulle.
*/
function columnChart(container, bars, options = {}) {
  const width = options.width || 720;
  const height = options.height || 190;
  const padLeft = options.axis === false ? 0 : 40;
  const padBottom = options.labels === false ? 4 : 22;
  const padTop = 10;
  const plotWidth = width - padLeft;
  const plotHeight = height - padTop - padBottom;
  const max = niceMax(Math.max(1, ...bars.map((bar) => bar.value), options.reference || 0));
  const slot = plotWidth / bars.length;
  const barWidth = Math.min(24, Math.max(3, slot - 2 - Math.min(10, slot * 0.28)));
  const y = (value) => padTop + plotHeight - (value / max) * plotHeight;

  let svg = `<svg viewBox="0 0 ${width} ${height}" role="img" aria-label="${T.escape(options.label || "")}">`;

  if (options.axis !== false) {
    for (const fraction of [0.5, 1]) {
      const value = max * fraction;
      svg += `<line class="gridline" x1="${padLeft}" x2="${width}" y1="${y(value)}" y2="${y(value)}"/>`;
      svg += `<text class="tick" x="${padLeft - 8}" y="${y(value) + 3.5}" text-anchor="end">${tickLabel(value)}</text>`;
    }
  }

  bars.forEach((bar, index) => {
    const x = padLeft + index * slot + (slot - barWidth) / 2;
    const top = y(bar.value);
    svg += `<g class="slot" data-index="${index}">`;
    svg += `<rect class="hit${bar.onClick ? " clickable" : ""}" x="${padLeft + index * slot}" y="${padTop}" width="${slot}" height="${plotHeight + padBottom}"/>`;
    if (bar.value > 0) {
      svg += `<path class="mark${bar.dim ? " dim" : ""}" d="${columnPath(x, top, barWidth, padTop + plotHeight - top)}"/>`;
    }
    if (bar.label && options.labels !== false) {
      svg += `<text class="tick" x="${x + barWidth / 2}" y="${height - 6}" text-anchor="middle">${T.escape(bar.label)}</text>`;
    }
    svg += "</g>";
  });

  svg += `<line class="baseline" x1="${padLeft}" x2="${width}" y1="${padTop + plotHeight + 0.5}" y2="${padTop + plotHeight + 0.5}"/>`;

  if (options.reference) {
    const ry = y(options.reference);
    svg += `<line class="reference" x1="${padLeft}" x2="${width}" y1="${ry}" y2="${ry}"/>`;
    svg += `<text class="reference-label" x="${width}" y="${ry - 5}" text-anchor="end">${T.escape(options.referenceLabel || "")}</text>`;
  }

  svg += "</svg>";
  container.innerHTML = `<div class="chart">${svg}</div>`;

  container.querySelectorAll(".slot").forEach((group) => {
    const bar = bars[Number(group.dataset.index)];
    if (bar.tip) T.tip(group, bar.tip);
    if (bar.onClick) group.addEventListener("click", bar.onClick);
  });
}

/*
  Chronologie : une ligne par application, l'axe des heures en abscisse.
  « Ouvert » en voile léger, « au premier plan » en plein.
*/
function timeline(container, day, keys, options = {}) {
  const labelWidth = 170;
  const rowHeight = 30;
  const width = 900;
  const plotWidth = width - labelWidth - 8;
  const axisHeight = 22;
  const height = keys.length * rowHeight + axisHeight;
  const x = (second) => labelWidth + (second / 86400) * plotWidth;

  let svg = `<svg viewBox="0 0 ${width} ${height}" role="img" aria-label="Chronologie de la journée">`;

  for (let hour = 0; hour <= 24; hour += 3) {
    const gx = x(hour * 3600);
    svg += `<line class="gridline" x1="${gx}" x2="${gx}" y1="0" y2="${keys.length * rowHeight}"/>`;
    svg += `<text class="tick" x="${gx}" y="${height - 6}" text-anchor="middle">${hour}${NB}h</text>`;
  }

  keys.forEach((key, row) => {
    const top = row * rowHeight;
    const sessions = day.sessions.filter((span) => span.app === key);
    const focus = day.focus.filter((span) => span.app === key);
    for (const span of sessions) {
      svg += `<rect class="mark-wash" data-kind="open" data-app="${T.escape(key)}" data-start="${span.start}" data-end="${span.end}" x="${x(span.start)}" y="${top + 7}" width="${Math.max(1.5, x(span.end) - x(span.start))}" height="${rowHeight - 14}" rx="3"/>`;
    }
    for (const span of focus) {
      svg += `<rect class="mark" data-kind="focus" data-app="${T.escape(key)}" data-start="${span.start}" data-end="${span.end}" x="${x(span.start)}" y="${top + 9}" width="${Math.max(1.5, x(span.end) - x(span.start))}" height="${rowHeight - 18}" rx="2"/>`;
    }
  });

  if (options.now != null) {
    const nx = x(options.now);
    svg += `<line class="now-line" x1="${nx}" x2="${nx}" y1="0" y2="${keys.length * rowHeight}"/>`;
  }

  svg += "</svg>";

  const labels = keys.map((key, row) => `
    <div style="position:absolute;left:0;top:calc(${(row * rowHeight + rowHeight / 2) / height * 100}% - 11px);width:${labelWidth / width * 100}%;display:flex;align-items:center;gap:8px;padding-right:12px">
      ${appIcon(key)}<span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12px">${T.escape(appName(key))}</span>
    </div>`).join("");

  container.innerHTML = `<div class="chart" style="position:relative">${svg}${labels}</div>`;

  container.querySelectorAll("rect[data-kind]").forEach((rect) => {
    T.tip(rect, () => {
      const start = Number(rect.dataset.start);
      const end = Number(rect.dataset.end);
      const kind = rect.dataset.kind === "focus" ? "Au premier plan" : "Ouvert";
      return `<div class="tooltip-title">${T.escape(appName(rect.dataset.app))}</div>
        <div class="tooltip-row"><span class="grow">${kind}</span><b>${T.duration(end - start)}</b></div>
        <div class="tooltip-row"><span class="grow">De ${T.clock(start)} à ${T.clock(end)}</span></div>`;
    });
  });
}

/* Classement horizontal : icône, nom, barre, valeur. */
function ranking(container, rows, options = {}) {
  const max = Math.max(1, ...rows.map((row) => row.value));
  container.innerHTML = `<div class="rank">${rows.map((row, index) => `
    <div class="rank-row${options.onClick ? " clickable" : ""}" data-index="${index}">
      ${appIcon(row.key)}
      <div class="rank-name">${T.escape(appName(row.key))}${row.note ? `<small>${row.note}</small>` : ""}</div>
      <div class="rank-track">${row.value > 0 ? `<div class="rank-bar" style="width:${(row.value / max) * 100}%"></div>` : ""}</div>
      <div class="rank-value">${T.duration(row.value)}</div>
    </div>`).join("")}</div>`;

  container.querySelectorAll(".rank-row").forEach((element) => {
    const row = rows[Number(element.dataset.index)];
    if (row.tip) T.tip(element, row.tip);
    if (options.onClick) element.addEventListener("click", () => options.onClick(row));
  });
}

function hourLabel(hour) {
  return `De ${hour}${NB}h à ${hour + 1}${NB}h`;
}

function stateLabel(state) {
  return {
    ok: `${T.icon("check", 13)}Dans le budget`,
    soon: `${T.icon("alert", 13)}Bientôt atteint`,
    over: `${T.icon("ban", 13)}Dépassé`,
  }[state];
}

function metricLabel(metric) {
  return metric === "open" ? "ouvert" : "au premier plan";
}

function actionLabel(action) {
  return action === "close" ? "Fermer l’application" : "Prévenir";
}

function budgetMeter(status) {
  const ratio = Math.min(1, status.used / Math.max(1, status.limit));
  return `<div class="meter ${status.state}"><div class="meter-fill" style="width:${ratio * 100}%"></div></div>`;
}

function nowSeconds() {
  const now = new Date();
  return now.getHours() * 3600 + now.getMinutes() * 60 + now.getSeconds();
}

function sinceText(milliseconds) {
  if (!milliseconds) return "";
  return T.duration((Date.now() - milliseconds) / 1000);
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

/* --- Page : la journée --------------------------------------------------- */

async function renderDay(root, ctx) {
  let date = ctx.params.date || T.isoDate(new Date());

  async function draw() {
    const day = await ctx.request("day", { date });
    await ensureCatalog(ctx.request, day.catalogVersion);
    const today = T.isoDate(new Date());
    const title = day.isToday ? "Aujourd’hui" : T.capitalize(T.dateLabel(date, "weekdayLong"));
    const subtitle = T.capitalize(T.dateLabel(date));

    const comparison = T.delta(day.total, day.average7);
    const deltaHtml = comparison
      ? `<span class="delta ${comparison.change > 2 ? "up" : comparison.change < -2 ? "down" : "flat"}">${T.icon(comparison.change >= 0 ? "arrowUp" : "arrowDown", 13)}${comparison.text}</span> <span class="card-sub">par rapport à la moyenne des 7${NB}jours précédents (${T.duration(day.average7)})</span>`
      : `<span class="card-sub">Pas encore assez d’historique pour comparer.</span>`;

    const top = day.apps[0];
    const firstFocus = day.focus.length ? day.focus[0].start : null;
    const lastFocus = day.focus.length ? day.focus[day.focus.length - 1].end : null;

    root.innerHTML = pageHead(title, subtitle, `
        <button class="btn btn-icon" id="previous" title="Jour précédent" type="button">${T.icon("chevronLeft", 17)}</button>
        <button class="btn" id="todayButton" type="button" ${day.isToday ? "disabled" : ""}>Aujourd’hui</button>
        <button class="btn btn-icon" id="next" title="Jour suivant" type="button" ${date >= today ? "disabled" : ""}>${T.icon("chevronRight", 17)}</button>`) + `
      <div class="stack">
        <section class="card" style="display:grid;grid-template-columns:minmax(0,1.1fr) repeat(3,minmax(0,1fr));gap:22px;align-items:end">
          <div>
            <div class="stat-label">Temps d’écran${day.isToday && day.live && day.live.paused ? `${T.sep}suivi suspendu` : ""}</div>
            <div class="hero" style="margin:6px 0 10px">${T.durationHtml(day.total)}</div>
            <div>${deltaHtml}</div>
          </div>
          <div>
            <div class="stat-label">Application la plus utilisée</div>
            <div style="display:flex;align-items:center;gap:8px;margin-top:6px">${top ? `${appIcon(top.key, "md")}<div style="min-width:0"><div style="font-weight:650;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${T.escape(appName(top.key))}</div><div class="stat-note">${T.duration(top.active)}</div></div>` : `<div class="stat-note">${T.none}</div>`}</div>
          </div>
          <div>
            <div class="stat-label">Lancements</div>
            <div class="stat-value">${day.launches}</div>
            <div class="stat-note">${day.apps.length}${NB}application${day.apps.length > 1 ? "s" : ""} utilisée${day.apps.length > 1 ? "s" : ""}</div>
          </div>
          <div>
            <div class="stat-label">Activité</div>
            <div class="stat-value num">${firstFocus != null ? `${T.clock(firstFocus)}<span class="range-arrow">${T.icon("arrowRight", 16)}</span>${T.clock(lastFocus)}` : T.none}</div>
            <div class="stat-note">Première et dernière utilisation</div>
          </div>
        </section>

        <section class="card">
          <div class="card-head"><div class="card-title">Heure par heure</div><div class="spacer"></div><div class="card-sub">Temps au premier plan</div></div>
          <div id="hours"></div>
        </section>

        <section class="card">
          <div class="card-head">
            <div class="card-title">Chronologie</div>
            <div class="spacer"></div>
            <div class="legend">
              <span class="legend-key"><span class="legend-swatch"></span>Au premier plan</span>
              <span class="legend-key"><span class="legend-swatch wash"></span>Ouvert</span>
            </div>
          </div>
          <div id="timeline"></div>
        </section>

        <section class="card">
          <div class="card-head"><div class="card-title">Applications</div><div class="spacer"></div><div class="card-sub">Cliquez pour le détail</div></div>
          <div id="ranking"></div>
        </section>
      </div>`;

    root.querySelector("#previous").addEventListener("click", () => { date = T.addDays(date, -1); draw(); });
    root.querySelector("#next").addEventListener("click", () => { date = T.addDays(date, 1); draw(); });
    root.querySelector("#todayButton").addEventListener("click", () => { date = today; draw(); });

    const currentHour = day.isToday ? new Date().getHours() : 24;
    columnChart(root.querySelector("#hours"), day.hours.map((value, hour) => ({
      value,
      dim: hour > currentHour,
      label: hour % 3 === 0 ? `${hour}${NB}h` : "",
      tip: () => {
        const apps = day.apps
          .map((app) => ({ key: app.key, value: app.hours[hour] }))
          .filter((app) => app.value > 0)
          .sort((a, b) => b.value - a.value)
          .slice(0, 4);
        return `<div class="tooltip-title">${hourLabel(hour)}</div>
          <div class="tooltip-row"><span class="grow">Total</span><b>${T.duration(value)}</b></div>
          ${apps.map((app) => `<div class="tooltip-row">${appIcon(app.key)}<span class="grow">${T.escape(appName(app.key))}</span><b>${T.duration(app.value)}</b></div>`).join("")}`;
      },
    })), { height: 200, label: "Temps au premier plan, heure par heure" });

    const visibleKeys = day.apps.filter((app) => app.active > 0 || app.open > 0).slice(0, 8).map((app) => app.key);
    if (visibleKeys.length) {
      timeline(root.querySelector("#timeline"), day, visibleKeys, { now: day.isToday ? nowSeconds() : null });
    } else {
      root.querySelector("#timeline").innerHTML = `<div class="empty">${T.icon("clock", 26)}Rien d’enregistré pour cette journée.</div>`;
    }

    const rows = day.apps.filter((app) => app.active > 0 || app.open > 0).map((app) => ({
      key: app.key,
      value: app.active,
      note: `ouvert ${T.duration(app.open)}${app.launches ? `${T.sep}${app.launches}${NB}lancement${app.launches > 1 ? "s" : ""}` : ""}`,
    }));
    if (rows.length) {
      ranking(root.querySelector("#ranking"), rows, { onClick: (row) => ctx.navigate("app", { key: row.key }) });
    } else {
      root.querySelector("#ranking").innerHTML = `<div class="empty">${T.icon("apps", 26)}Aucune application pour l’instant.</div>`;
    }
  }

  await draw();
  ctx.every(15, () => { if (date === T.isoDate(new Date())) draw(); });
  ctx.on("screen-time.day-changed", () => draw());
}

/* --- Page : historique --------------------------------------------------- */

async function renderHistory(root, ctx) {
  let days = Number(ctx.params.days) || 7;

  async function draw() {
    const range = await ctx.request("range", { days });
    await ensureCatalog(ctx.request, range.catalogVersion);
    const comparison = T.delta(range.total, range.previousTotal);

    root.innerHTML = pageHead("Historique", "Le temps au premier plan, jour après jour.", `
      <div class="segmented" id="period">
        ${[7, 30, 90].map((value) => `<button type="button" data-days="${value}" aria-pressed="${value === days}">${value}${NB}jours</button>`).join("")}
      </div>`) + `
      <div class="stack">
        <section class="card" style="display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:22px;align-items:end">
          <div>
            <div class="stat-label">Total sur ${days}${NB}jours</div>
            <div class="hero" style="margin-top:6px">${T.durationHtml(range.total)}</div>
          </div>
          <div>
            <div class="stat-label">Moyenne par jour</div>
            <div class="stat-value">${T.durationHtml(range.average)}</div>
            <div class="stat-note">sur ${range.recordedDays}${NB}jour${range.recordedDays > 1 ? "s" : ""} enregistré${range.recordedDays > 1 ? "s" : ""}</div>
          </div>
          <div>
            <div class="stat-label">Période précédente</div>
            <div class="stat-value">${T.durationHtml(range.previousTotal)}</div>
            <div class="stat-note">${comparison ? `<span class="delta ${comparison.change > 2 ? "up" : comparison.change < -2 ? "down" : "flat"}">${comparison.text}</span> cette période` : "Pas de comparaison possible"}</div>
          </div>
        </section>

        <section class="card">
          <div class="card-head"><div class="card-title">Par jour</div><div class="spacer"></div><div class="card-sub">Cliquez sur un jour pour l’ouvrir</div></div>
          <div id="daily"></div>
        </section>

        <section class="card">
          <div class="card-head"><div class="card-title">Applications les plus utilisées</div></div>
          <div id="ranking"></div>
        </section>
      </div>`;

    root.querySelectorAll("#period button").forEach((button) =>
      button.addEventListener("click", () => { days = Number(button.dataset.days); draw(); }));

    const labelEvery = days <= 7 ? 1 : days <= 30 ? 5 : 15;
    columnChart(root.querySelector("#daily"), range.days.map((day, index) => ({
      value: day.total,
      label: index % labelEvery === 0 || index === range.days.length - 1
        ? (days <= 7 ? T.capitalize(T.dateLabel(day.date, "weekday")) : T.dateLabel(day.date, "short"))
        : "",
      tip: `<div class="tooltip-title">${T.capitalize(T.dateLabel(day.date))}</div>
        <div class="tooltip-row"><span class="grow">Temps d’écran</span><b>${T.duration(day.total)}</b></div>`,
      onClick: () => ctx.navigate("today", { date: day.date }),
    })), {
      height: 220,
      reference: range.average,
      referenceLabel: `moyenne ${T.duration(range.average)}`,
      label: "Temps d’écran par jour",
    });

    const rows = range.apps.filter((app) => app.active > 0).slice(0, 15).map((app) => ({
      key: app.key,
      value: app.active,
      note: `${app.days}${NB}jour${app.days > 1 ? "s" : ""}${T.sep}${T.duration(app.active / Math.max(1, app.days))} par jour d’usage`,
    }));
    if (rows.length) {
      ranking(root.querySelector("#ranking"), rows, { onClick: (row) => ctx.navigate("app", { key: row.key }) });
    } else {
      root.querySelector("#ranking").innerHTML = `<div class="empty">${T.icon("chart", 26)}Pas encore d’historique sur cette période.</div>`;
    }
  }

  await draw();
}

/* --- Page : toutes les applications ------------------------------------- */

async function renderApps(root, ctx) {
  const result = await ctx.request("apps");
  await ensureCatalog(ctx.request, result.catalogVersion);
  let sort = "total";
  let query = "";
  let showHidden = false;

  root.innerHTML = pageHead("Applications", "Toutes les applications vues par Atelier, avec leur temps cumulé.", `
      <input class="input" id="search" placeholder="Rechercher…" style="width:220px" />
      <button class="btn" id="hiddenToggle" type="button">${T.icon("eye", 15)}<span>Afficher les masquées</span></button>`) + `
    <section class="card" style="padding:8px 8px">
      <div id="table"></div>
    </section>`;

  const columns = [
    { id: "name", label: "Application" },
    { id: "today", label: "Aujourd’hui" },
    { id: "week", label: "7 derniers jours" },
    { id: "total", label: "Total" },
    { id: "open", label: "Ouvert (total)" },
    { id: "lastUsed", label: "Dernière utilisation" },
  ];

  function draw() {
    const list = result.apps
      .filter((app) => showHidden || !(catalog.apps[app.key] && catalog.apps[app.key].hidden))
      .filter((app) => !query || appName(app.key).toLowerCase().includes(query))
      .sort((a, b) => sort === "name"
        ? appName(a.key).localeCompare(appName(b.key), "fr")
        : sort === "lastUsed"
          ? String(b.lastUsed || "").localeCompare(String(a.lastUsed || ""))
          : (b[sort] || 0) - (a[sort] || 0));

    const cell = "padding:9px 12px;text-align:right;font-variant-numeric:tabular-nums;color:var(--text-2)";
    root.querySelector("#table").innerHTML = list.length ? `
      <table style="width:100%;border-collapse:collapse;font-size:12.5px">
        <thead><tr>${columns.map((column) => `
          <th data-sort="${column.id}" style="padding:9px 12px;text-align:${column.id === "name" ? "left" : "right"};font-size:11.5px;font-weight:650;color:${sort === column.id ? "var(--text)" : "var(--muted)"};cursor:pointer;white-space:nowrap">${column.label}${sort === column.id ? T.icon("arrowDown", 12, 'style="display:inline;vertical-align:-1px;margin-left:4px"') : ""}</th>`).join("")}</tr></thead>
        <tbody>${list.map((app) => `
          <tr data-key="${T.escape(app.key)}" style="cursor:pointer;border-top:1px solid var(--border)">
            <td style="padding:8px 12px"><div style="display:flex;align-items:center;gap:10px;min-width:0">${appIcon(app.key)}<span style="font-weight:550;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${T.escape(appName(app.key))}</span>${catalog.apps[app.key] && catalog.apps[app.key].hidden ? `<span class="badge">masquée</span>` : ""}</div></td>
            <td style="${cell}">${app.today ? T.duration(app.today) : T.none}</td>
            <td style="${cell}">${app.week ? T.duration(app.week) : T.none}</td>
            <td style="${cell};color:var(--text);font-weight:600">${app.total ? T.duration(app.total) : T.none}</td>
            <td style="${cell}">${app.open ? T.duration(app.open) : T.none}</td>
            <td style="${cell}">${app.lastUsed ? T.dateLabel(app.lastUsed, "short") : T.none}</td>
          </tr>`).join("")}</tbody>
      </table>` : `<div class="empty">${T.icon("search", 26)}Aucune application ne correspond.</div>`;

    root.querySelectorAll("th[data-sort]").forEach((th) =>
      th.addEventListener("click", () => { sort = th.dataset.sort; draw(); }));
    root.querySelectorAll("tr[data-key]").forEach((row) => {
      row.addEventListener("mouseenter", () => { row.style.background = "var(--surface-2)"; });
      row.addEventListener("mouseleave", () => { row.style.background = ""; });
      row.addEventListener("click", () => ctx.navigate("app", { key: row.dataset.key }));
    });
  }

  root.querySelector("#search").addEventListener("input", (event) => {
    query = event.target.value.trim().toLowerCase();
    draw();
  });
  root.querySelector("#hiddenToggle").addEventListener("click", (event) => {
    showHidden = !showHidden;
    event.currentTarget.querySelector("span").textContent = showHidden ? "Masquer les masquées" : "Afficher les masquées";
    draw();
  });
  draw();
}

/* --- Page : fiche d'une application ------------------------------------- */

async function renderApp(root, ctx) {
  const key = ctx.params.key;

  async function draw() {
    const app = await ctx.request("app", { key });
    await ensureCatalog(ctx.request, app.catalogVersion);
    const info = app.info || {};
    const week = app.days.slice(-7).reduce((sum, day) => sum + day.active, 0);
    const month = app.days.reduce((sum, day) => sum + day.active, 0);
    const budget = app.budget;

    root.innerHTML = `
      <div class="page-head">
        <button class="btn btn-icon" id="back" type="button" title="Retour">${T.icon("chevronLeft", 17)}</button>
        ${appIcon(key, "lg")}
        <div class="titles">
          <h1 class="page-title">${T.escape(appName(key))}</h1>
          <p class="page-lede" style="font-family:var(--mono);font-size:11px;color:var(--muted);user-select:text">${T.escape(info.path || key)}</p>
        </div>
        <div class="page-actions">
          <button class="btn" id="rename" type="button">${T.icon("edit", 15)}Renommer</button>
          <button class="btn" id="hide" type="button">${T.icon(info.hidden ? "eye" : "eyeOff", 15)}${info.hidden ? "Réafficher" : "Masquer"}</button>
          <button class="btn btn-primary" id="budget" type="button">${T.icon("target", 15)}${budget ? "Modifier le budget" : "Fixer un budget"}</button>
        </div>
      </div>
      <div class="stack">
        <section class="card grid-3" style="display:grid">
          <div>
            <div class="stat-label">Aujourd’hui</div>
            <div class="stat-value">${T.durationHtml(app.today.active)}</div>
            <div class="stat-note">ouvert ${T.duration(app.today.open)}${T.sep}${app.today.launches}${NB}lancement${app.today.launches > 1 ? "s" : ""}</div>
          </div>
          <div>
            <div class="stat-label">7 derniers jours</div>
            <div class="stat-value">${T.durationHtml(week)}</div>
            <div class="stat-note">${T.duration(week / 7)} par jour en moyenne</div>
          </div>
          <div>
            <div class="stat-label">${app.runningSince ? "Lancée depuis" : "30 derniers jours"}</div>
            <div class="stat-value">${app.runningSince ? T.escape(sinceText(app.runningSince)) : T.durationHtml(month)}</div>
            <div class="stat-note">${app.runningSince ? `<span class="badge live">en cours</span> depuis ${new Date(app.runningSince).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" })}` : "au premier plan"}</div>
          </div>
        </section>

        ${budget ? `
        <section class="card">
          <div class="card-head"><div class="card-title">Budget du jour</div><div class="spacer"></div><span class="state ${budget.state}">${stateLabel(budget.state)}</span></div>
          ${budgetMeter(budget)}
          <div class="stat-note" style="margin-top:8px">${T.duration(budget.used)} sur ${T.duration(budget.limit)} ${metricLabel(budget.metric)}${budget.extension ? ` (dont ${budget.extension}${NB}min de rallonge)` : ""}${T.sep}${actionLabel(budget.action)}</div>
        </section>` : ""}

        <section class="card">
          <div class="card-head"><div class="card-title">30 derniers jours</div><div class="spacer"></div><div class="card-sub">Temps au premier plan</div></div>
          <div id="days"></div>
        </section>

        <section class="card">
          <div class="card-head"><div class="card-title">Aujourd’hui, heure par heure</div></div>
          <div id="hours"></div>
        </section>

        <section class="card">
          <div class="card-head"><div class="card-title">Sessions d’aujourd’hui</div><div class="spacer"></div><div class="card-sub">Du lancement à la fermeture</div></div>
          <div id="sessions"></div>
        </section>
      </div>`;

    root.querySelector("#back").addEventListener("click", () => history.length > 1 ? ctx.navigate("today") : ctx.navigate("apps"));
    root.querySelector("#budget").addEventListener("click", () => editBudget(ctx, key, budget, draw));
    root.querySelector("#hide").addEventListener("click", async () => {
      await ctx.request("app.hide", { key, hidden: !info.hidden });
      catalog.version = -1;
      T.toast(info.hidden ? "L’application réapparaît dans les statistiques." : "L’application est masquée des statistiques. Son temps reste enregistré.", "success");
      draw();
    });
    root.querySelector("#rename").addEventListener("click", () => {
      T.dialog({
        title: "Renommer l’application",
        subtitle: `Nom d’origine : ${info.originalName || appName(key)}. Laissez vide pour le rétablir.`,
        build(body) {
          body.innerHTML = `<input class="input" id="newName" value="${T.escape(info.name || "")}" />`;
        },
        actions: [
          { label: "Annuler", ghost: true },
          {
            label: "Renommer",
            primary: true,
            run: async (close) => {
              const name = document.getElementById("newName").value;
              await ctx.request("app.rename", { key, name });
              catalog.version = -1;
              close();
              draw();
            },
          },
        ],
      });
    });

    columnChart(root.querySelector("#days"), app.days.map((day, index) => ({
      value: day.active,
      label: index % 5 === 0 || index === app.days.length - 1 ? T.dateLabel(day.date, "short") : "",
      tip: `<div class="tooltip-title">${T.capitalize(T.dateLabel(day.date))}</div>
        <div class="tooltip-row"><span class="grow">Au premier plan</span><b>${T.duration(day.active)}</b></div>
        <div class="tooltip-row"><span class="grow">Ouvert</span><b>${T.duration(day.open)}</b></div>`,
      onClick: () => ctx.navigate("today", { date: day.date }),
    })), { height: 180, label: "Temps au premier plan, 30 derniers jours" });

    const currentHour = new Date().getHours();
    columnChart(root.querySelector("#hours"), app.today.hours.map((value, hour) => ({
      value,
      dim: hour > currentHour,
      label: hour % 3 === 0 ? `${hour}${NB}h` : "",
      tip: `<div class="tooltip-title">${hourLabel(hour)}</div><div class="tooltip-row"><span class="grow">Au premier plan</span><b>${T.duration(value)}</b></div>`,
    })), { height: 160, label: "Temps au premier plan, heure par heure" });

    const sessions = root.querySelector("#sessions");
    if (app.sessions.length) {
      sessions.innerHTML = `<div class="rank">${app.sessions.slice().reverse().map((session) => {
        const focused = app.focus
          .filter((span) => span.end > session.start && span.start < session.end)
          .reduce((sum, span) => sum + Math.min(span.end, session.end) - Math.max(span.start, session.start), 0);
        return `<div class="rank-row" style="grid-template-columns:minmax(0,1fr) auto auto">
          <div class="rank-name num">${T.clock(session.start)}<span class="range-arrow">${T.icon("arrowRight", 13)}</span>${T.clock(session.end)}</div>
          <div class="rank-value" style="width:auto">${T.duration(focused)} au premier plan</div>
          <div class="rank-value" style="width:90px;color:var(--text);font-weight:600">${T.duration(session.end - session.start)}</div>
        </div>`;
      }).join("")}</div>`;
    } else {
      sessions.innerHTML = `<div class="empty">${T.icon("clock", 26)}Pas lancée aujourd’hui.</div>`;
    }
  }

  await draw();
  ctx.every(15, draw);
}

/* --- Budgets ------------------------------------------------------------- */

function editBudget(ctx, key, existing, done) {
  const minutes = existing ? existing.minutes : 60;
  const metric = existing ? existing.metric : "active";
  const action = existing ? existing.action : "notify";
  const keys = Object.keys(catalog.apps)
    .filter((candidate) => !catalog.apps[candidate].hidden || candidate === key)
    .sort((a, b) => appName(a).localeCompare(appName(b), "fr"));

  T.dialog({
    title: existing ? "Modifier le budget" : "Nouveau budget",
    subtitle: "Une durée quotidienne, remise à zéro chaque nuit à minuit.",
    build(body) {
      body.innerHTML = `
        <div class="field">
          <label class="field-label" for="budgetApp">Application</label>
          <select class="input" id="budgetApp" ${key ? "disabled" : ""}>
            ${keys.map((candidate) => `<option value="${T.escape(candidate)}" ${candidate === key ? "selected" : ""}>${T.escape(appName(candidate))}</option>`).join("")}
          </select>
        </div>
        <div class="field">
          <span class="field-label">Durée par jour</span>
          <div class="input-group">
            <input class="input" id="budgetHours" inputmode="numeric" value="${Math.floor(minutes / 60)}" /><span class="input-suffix">h</span>
            <input class="input" id="budgetMinutes" inputmode="numeric" value="${minutes % 60}" /><span class="input-suffix">min</span>
          </div>
        </div>
        <div class="field">
          <span class="field-label">Ce qui est compté</span>
          <div class="choices">
            <label class="choice"><input type="radio" name="metric" value="active" ${metric === "active" ? "checked" : ""} /><span class="choice-dot"></span>
              <span><span class="choice-title">Le temps au premier plan</span><span class="choice-desc">Quand l’application est devant vous et que vous êtes là.</span></span></label>
            <label class="choice"><input type="radio" name="metric" value="open" ${metric === "open" ? "checked" : ""} /><span class="choice-dot"></span>
              <span><span class="choice-title">Le temps où elle est lancée</span><span class="choice-desc">Même réduite ou en arrière-plan. Plus strict.</span></span></label>
          </div>
        </div>
        <div class="field" style="margin-bottom:0">
          <span class="field-label">Quand le budget est épuisé</span>
          <div class="choices">
            <label class="choice"><input type="radio" name="action" value="notify" ${action === "notify" ? "checked" : ""} /><span class="choice-dot"></span>
              <span><span class="choice-title">Prévenir</span><span class="choice-desc">Une notification à l’approche puis à l’épuisement, et des rappels ensuite.</span></span></label>
            <label class="choice"><input type="radio" name="action" value="close" ${action === "close" ? "checked" : ""} /><span class="choice-dot"></span>
              <span><span class="choice-title">Fermer l’application</span><span class="choice-desc">Après un compte à rebours. Une rallonge reste possible depuis la notification.</span></span></label>
          </div>
        </div>`;
    },
    actions: [
      ...(existing ? [{
        label: "Supprimer",
        danger: true,
        run: async (close) => {
          await ctx.request("budget.delete", { key });
          close();
          T.toast("Budget supprimé.", "success");
          done();
        },
      }, { spacer: true }] : []),
      { label: "Annuler", ghost: true },
      {
        label: "Enregistrer",
        primary: true,
        run: async (close) => {
          const hours = parseInt(document.getElementById("budgetHours").value, 10) || 0;
          const mins = parseInt(document.getElementById("budgetMinutes").value, 10) || 0;
          const total = hours * 60 + mins;
          if (total <= 0) {
            T.toast("Indiquez une durée d’au moins une minute.", "error");
            return;
          }
          const budget = {
            app: key || document.getElementById("budgetApp").value,
            minutes: total,
            metric: document.querySelector('input[name="metric"]:checked').value,
            action: document.querySelector('input[name="action"]:checked').value,
            enabled: existing ? existing.enabled : true,
          };
          try {
            await ctx.request("budget.save", { budget });
            close();
            T.toast(`Budget enregistré : ${T.duration(total * 60)} par jour pour ${appName(budget.app)}.`, "success");
            done();
          } catch (error) {
            T.toast(error.message, "error");
          }
        },
      },
    ],
  });
}

async function renderBudgets(root, ctx) {
  async function draw() {
    const result = await ctx.request("budgets");
    await ensureCatalog(ctx.request, result.catalogVersion);
    const settings = await ctx.request("settings");

    root.innerHTML = pageHead("Budgets",
      `Une durée par jour et par application. Atelier prévient ${settings.warn_minutes}${NB}min avant l’épuisement ; selon le budget, il rappelle ensuite à l’ordre ou ferme l’application après ${settings.close_grace_seconds}${NB}s de compte à rebours.`,
      `<button class="btn btn-primary" id="add" type="button">${T.icon("plus", 15)}Nouveau budget</button>`) +
      '<div class="stack" id="list"></div>';

    root.querySelector("#add").addEventListener("click", () => editBudget(ctx, null, null, draw));
    const list = root.querySelector("#list");

    if (!result.budgets.length) {
      list.innerHTML = `
        <section class="card"><div class="empty">${T.icon("target", 30)}
          <div style="color:var(--text);font-weight:650;font-size:14px">Aucun budget</div>
          <div style="max-width:52ch">Fixez par exemple 1${NB}h30 de jeu par jour : Atelier vous prévient à l’approche, puis ferme le jeu si vous le souhaitez.</div>
        </div></section>`;
      return;
    }

    for (const status of result.budgets) {
      const card = document.createElement("section");
      card.className = "card";
      const remaining = Math.max(0, status.limit - status.used);
      card.innerHTML = `
        <div class="card-head" style="margin-bottom:12px">
          ${appIcon(status.app, "md")}
          <div style="min-width:0;flex:1">
            <div class="card-title">${T.escape(appName(status.app))}</div>
            <div class="card-sub">${T.duration(status.minutes * 60)} par jour, ${metricLabel(status.metric)}${T.sep}${actionLabel(status.action)}</div>
          </div>
          <span class="state ${status.enabled ? status.state : "ok"}">${status.enabled ? stateLabel(status.state) : "En pause"}</span>
          <label class="toggle" style="padding:0;border:0" title="${status.enabled ? "Désactiver" : "Activer"}">
            <input type="checkbox" ${status.enabled ? "checked" : ""} />
            <span class="track"><span class="thumb"></span></span>
          </label>
          <button class="btn-icon" type="button" title="Modifier" data-edit>${T.icon("edit", 15)}</button>
        </div>
        ${budgetMeter(status)}
        <div style="display:flex;justify-content:space-between;margin-top:8px;font-size:12px;color:var(--text-2)">
          <span class="num">${T.duration(status.used)} utilisé${status.extension ? `${T.sep}rallonge de ${status.extension}${NB}min` : ""}</span>
          <span class="num">${status.state === "over" ? `dépassé de ${T.duration(status.used - status.limit)}` : `reste ${T.duration(remaining)}`}</span>
        </div>`;
      card.querySelector("[data-edit]").addEventListener("click", () => editBudget(ctx, status.app, status, draw));
      card.querySelector("input[type=checkbox]").addEventListener("change", async (event) => {
        const budget = { app: status.app, minutes: status.minutes, metric: status.metric, action: status.action, enabled: event.target.checked };
        await ctx.request("budget.save", { budget });
        draw();
      });
      list.appendChild(card);
    }
  }

  await draw();
  ctx.every(10, draw);
}

/* --- Réglages du suivi --------------------------------------------------- */

async function renderSettings(root, ctx) {
  const settings = await ctx.request("settings");
  const live = await ctx.request("live");

  const number = (id, label, value, suffix, hint) => `
    <div class="field">
      <label class="field-label" for="${id}">${label}</label>
      <div class="input-group"><input class="input" id="${id}" inputmode="numeric" value="${value}" /><span class="input-suffix">${suffix}</span></div>
      ${hint ? `<div class="field-hint">${hint}</div>` : ""}
    </div>`;

  root.innerHTML = pageHead("Réglages du suivi", "Comment Atelier décide qu’une application est utilisée, et comment se comportent les budgets.") + `
    <div class="stack" style="max-width:760px">
      <section class="card">
        <div class="card-head"><div class="card-title">Suivi</div></div>
        <label class="toggle">
          <input type="checkbox" id="paused" ${live.paused ? "checked" : ""} />
          <span class="track"><span class="thumb"></span></span>
          <span class="toggle-text">
            <span class="toggle-title">Suspendre le suivi</span>
            <span class="toggle-desc">Plus rien n’est compté tant que l’interrupteur est actif. Reprend tout seul au prochain lancement d’Atelier.</span>
          </span>
        </label>
        <label class="toggle">
          <input type="checkbox" id="fullscreen" ${settings.fullscreen_counts_as_active ? "checked" : ""} />
          <span class="track"><span class="thumb"></span></span>
          <span class="toggle-text">
            <span class="toggle-title">Le plein écran compte comme une utilisation</span>
            <span class="toggle-desc">Une manette ou un film ne produisent aucune saisie aux yeux de Windows : sans cette option, une partie à la manette passerait pour une absence.</span>
          </span>
        </label>
        <div style="margin-top:14px">${number("idle", "Absence au bout de", settings.idle_minutes, "min sans clavier ni souris", "Au-delà, l’application au premier plan cesse d’être comptée.")}</div>
      </section>

      <section class="card">
        <div class="card-head"><div class="card-title">Budgets</div></div>
        <div class="grid-2">
          ${number("warn", "Prévenir avant l’épuisement", settings.warn_minutes, "min", "0 pour ne pas prévenir.")}
          ${number("extension", "Rallonge accordée", settings.extension_minutes, "min", "Bouton de la notification.")}
          ${number("grace", "Compte à rebours avant fermeture", settings.close_grace_seconds, "s", "De 10 à 600 secondes.")}
          ${number("reminder", "Rappel en cas de dépassement", settings.reminder_minutes, "min", "Budgets « Prévenir » uniquement.")}
        </div>
        <div class="card-actions"><button class="btn btn-primary" id="save" type="button">${T.icon("check", 15)}Enregistrer</button></div>
      </section>

      <section class="card">
        <div class="card-head"><div class="card-title">Données</div></div>
        <div class="field-hint" style="margin-top:0">Une journée par fichier JSON, lisible et exportable. Rien ne quitte cet ordinateur.</div>
        <div class="card-actions"><button class="btn" id="openData" type="button">${T.icon("folder", 15)}Ouvrir le dossier du module</button></div>
      </section>
    </div>`;

  root.querySelector("#paused").addEventListener("change", (event) =>
    ctx.request("pause", { paused: event.target.checked }));

  const read = (id) => parseInt(root.querySelector(`#${id}`).value, 10);
  async function save() {
    const next = {
      idle_minutes: read("idle"),
      fullscreen_counts_as_active: root.querySelector("#fullscreen").checked,
      warn_minutes: read("warn"),
      extension_minutes: read("extension"),
      close_grace_seconds: read("grace"),
      reminder_minutes: read("reminder"),
    };
    if (Object.values(next).some((value) => typeof value === "number" && !Number.isFinite(value))) {
      T.toast("Chaque durée doit être un nombre entier.", "error");
      return;
    }
    try {
      await ctx.request("settings.save", { settings: next });
      T.toast("Réglages enregistrés.", "success");
    } catch (error) {
      T.toast(error.message, "error");
    }
  }
  root.querySelector("#save").addEventListener("click", save);
  root.querySelector("#fullscreen").addEventListener("change", save);
  root.querySelector("#openData").addEventListener("click", () => ctx.request("open.data"));
}

/* --- Déclaration du module ---------------------------------------------- */

T.module({
  id: MODULE,
  name: "Temps d’écran",
  icon: "clock",
  description: "Temps passé dans chaque application, et budgets quotidiens.",
  pages: [
    { id: "today", title: "Journée", icon: "clock", render: renderDay },
    { id: "history", title: "Historique", icon: "chart", render: renderHistory },
    { id: "apps", title: "Applications", icon: "apps", render: renderApps },
    { id: "budgets", title: "Budgets", icon: "target", render: renderBudgets },
    { id: "settings", title: "Réglages du suivi", icon: "sliders", render: renderSettings },
    { id: "app", title: "Application", hidden: true, render: renderApp },
  ],
  /* Rappel discret, en bas de la barre latérale. */
  sidebar: {
    refresh: 5,
    async render(root, ctx) {
      const live = await ctx.request("live");
      await ensureCatalog(ctx.request, live.catalogVersion);
      if (live.paused) {
        root.innerHTML = `<div style="display:flex;align-items:center;gap:8px;color:var(--text-2)">${T.icon("pause", 14)}Suivi suspendu</div>`;
        return;
      }
      const key = live.foreground;
      root.innerHTML = `
        <div style="color:var(--muted);font-size:11px;margin-bottom:6px">Aujourd’hui${T.sep}<b style="color:var(--text)">${T.duration(live.total)}</b></div>
        ${key ? `<div style="display:flex;align-items:center;gap:8px;min-width:0">${appIcon(key)}<div style="min-width:0"><div style="font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${T.escape(appName(key))}</div>
          <div style="color:var(--muted);font-size:11px">${live.idle ? "en pause (absence)" : `${T.duration(live.foregroundToday)} aujourd’hui`}</div></div></div>` : `<div style="color:var(--muted)">Aucune application au premier plan</div>`}`;
    },
  },
});

/* --- Widgets ------------------------------------------------------------- */

T.widget({
  id: "screen-time.today",
  module: MODULE,
  title: "Temps d’écran",
  description: "Le total du jour, comparé à votre moyenne, et sa répartition par heure.",
  icon: "clock",
  sizes: ["m", "s", "l", "w"],
  refresh: 10,
  events: ["screen-time.day-changed"],
  async render(el, ctx) {
    const day = await ctx.request("day");
    await ensureCatalog(ctx.request, day.catalogVersion);
    const comparison = T.delta(day.total, day.average7);
    const deltaHtml = comparison
      ? `<span class="delta ${comparison.change > 2 ? "up" : comparison.change < -2 ? "down" : "flat"}">${T.icon(comparison.change >= 0 ? "arrowUp" : "arrowDown", 12)}${comparison.text}</span> vs moyenne`
      : "Aujourd’hui";

    if (ctx.size === "s") {
      el.innerHTML = `<div class="w-hero" style="font-size:36px">${T.durationHtml(day.total)}</div><div class="w-sub">${deltaHtml}</div>
        <div class="w-foot">${day.apps.length}${NB}application${day.apps.length > 1 ? "s" : ""}</div>`;
      return;
    }

    const showTop = ctx.size === "l";
    el.innerHTML = `
      <div style="display:flex;align-items:flex-end;gap:14px">
        <div><div class="w-hero">${T.durationHtml(day.total)}</div><div class="w-sub">${deltaHtml}</div></div>
      </div>
      <div class="w-fill" id="hours" style="margin-top:10px;display:flex;align-items:flex-end"></div>
      ${showTop ? '<div id="top" class="w-list" style="margin-top:12px"></div>' : ""}`;

    const currentHour = new Date().getHours();
    columnChart(el.querySelector("#hours"), day.hours.map((value, hour) => ({
      value,
      dim: hour > currentHour,
      label: hour % 6 === 0 ? `${hour}${NB}h` : "",
      tip: `<div class="tooltip-title">${hourLabel(hour)}</div><div class="tooltip-row"><span class="grow">Temps d’écran</span><b>${T.duration(value)}</b></div>`,
    })), { width: ctx.size === "w" ? 700 : 340, height: ctx.size === "l" ? 150 : 74, axis: false, label: "Temps d’écran par heure" });

    if (showTop) {
      const top = day.apps.filter((app) => app.active > 0).slice(0, 4);
      const max = Math.max(1, ...top.map((app) => app.active));
      el.querySelector("#top").innerHTML = top.map((app) => `
        <div class="w-row">${appIcon(app.key)}<span class="name">${T.escape(appName(app.key))}</span><span class="value">${T.duration(app.active)}</span>
        <span></span><div class="bar" style="width:${(app.active / max) * 100}%"></div></div>`).join("");
    }
  },
});

T.widget({
  id: "screen-time.now",
  module: MODULE,
  title: "En ce moment",
  description: "L’application au premier plan, depuis quand, et son total du jour.",
  icon: "activity",
  sizes: ["s", "m"],
  refresh: 2,
  async render(el, ctx) {
    const live = await ctx.request("live");
    await ensureCatalog(ctx.request, live.catalogVersion);
    const key = live.foreground;

    if (live.paused || !key) {
      el.innerHTML = `<div class="empty" style="padding:10px 0">${T.icon(live.paused ? "pause" : "monitor", 24)}${live.paused ? "Suivi suspendu" : "Aucune application au premier plan"}</div>`;
      return;
    }

    const since = live.foregroundSince != null ? nowSeconds() - live.foregroundSince : 0;
    const started = live.running[key];

    if (ctx.size === "s") {
      el.innerHTML = `
        <div style="display:flex;align-items:center;gap:9px">${appIcon(key, "md")}<div style="font-weight:650;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${T.escape(appName(key))}</div></div>
        <div class="w-hero" style="font-size:30px;margin-top:14px">${live.idle ? T.none : T.durationHtml(since)}</div>
        <div class="w-sub">${live.idle ? "Absence détectée" : "sans interruption"}</div>
        <div class="w-foot">${T.duration(live.foregroundToday)} aujourd’hui</div>`;
      return;
    }

    el.innerHTML = `
      <div style="display:flex;align-items:center;gap:12px">${appIcon(key, "lg")}
        <div style="min-width:0"><div style="font-weight:650;font-size:15px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${T.escape(appName(key))}</div>
        <div class="w-sub" style="margin-top:2px">${live.idle ? '<span class="badge">absence détectée</span>' : '<span class="badge live">au premier plan</span>'}</div></div>
      </div>
      <div style="display:grid;grid-template-columns:repeat(3,1fr);gap:10px;margin-top:auto">
        <div><div class="stat-label">Sans interruption</div><div class="stat-value" style="font-size:19px">${live.idle ? T.none : T.durationHtml(since)}</div></div>
        <div><div class="stat-label">Aujourd’hui</div><div class="stat-value" style="font-size:19px">${T.durationHtml(live.foregroundToday)}</div></div>
        <div><div class="stat-label">Lancée depuis</div><div class="stat-value" style="font-size:19px">${started ? T.durationHtml((Date.now() - started) / 1000) : T.none}</div></div>
      </div>`;
  },
});

T.widget({
  id: "screen-time.running",
  module: MODULE,
  title: "Applications lancées",
  description: "Ce qui tourne en ce moment, et depuis combien de temps.",
  icon: "hourglass",
  sizes: ["m", "l"],
  refresh: 5,
  async render(el, ctx) {
    const live = await ctx.request("live");
    await ensureCatalog(ctx.request, live.catalogVersion);
    const running = Object.entries(live.running)
      .filter(([key]) => !(catalog.apps[key] && catalog.apps[key].hidden))
      .sort((a, b) => (a[1] || Infinity) - (b[1] || Infinity));
    const limit = ctx.size === "l" ? 9 : 3;

    if (!running.length) {
      el.innerHTML = `<div class="empty">${T.icon("monitor", 24)}Aucune application lancée</div>`;
      return;
    }

    el.innerHTML = `<div class="w-list">${running.slice(0, limit).map(([key, started]) => `
      <div class="w-row">${appIcon(key)}<span class="name">${T.escape(appName(key))}${key === live.foreground ? ` <span class="badge live" style="height:16px;font-size:10px">actif</span>` : ""}</span>
      <span class="value">${started ? sinceText(started) : T.none}</span></div>`).join("")}</div>
      ${running.length > limit ? `<div class="w-foot">et ${running.length - limit}${NB}autre${running.length - limit > 1 ? "s" : ""}</div>` : ""}`;
  },
});

T.widget({
  id: "screen-time.top",
  module: MODULE,
  title: "Les plus utilisées",
  description: "Le classement des applications du jour.",
  icon: "apps",
  sizes: ["l", "m"],
  refresh: 15,
  events: ["screen-time.day-changed"],
  async render(el, ctx) {
    const day = await ctx.request("day");
    await ensureCatalog(ctx.request, day.catalogVersion);
    const apps = day.apps.filter((app) => app.active > 0).slice(0, ctx.size === "l" ? 8 : 3);
    if (!apps.length) {
      el.innerHTML = `<div class="empty">${T.icon("apps", 24)}Rien pour l’instant aujourd’hui</div>`;
      return;
    }
    const max = Math.max(1, ...apps.map((app) => app.active));
    el.innerHTML = `<div class="w-list" style="gap:${ctx.size === "l" ? 11 : 8}px">${apps.map((app) => `
      <div class="w-row">${appIcon(app.key)}<span class="name">${T.escape(appName(app.key))}</span><span class="value">${T.duration(app.active)}</span>
      <span></span><div class="bar" style="width:${(app.active / max) * 100}%"></div></div>`).join("")}</div>`;
  },
});

T.widget({
  id: "screen-time.week",
  module: MODULE,
  title: "Cette semaine",
  description: "Les sept derniers jours, et la moyenne quotidienne.",
  icon: "chart",
  sizes: ["m", "l", "w"],
  refresh: 60,
  events: ["screen-time.day-changed"],
  async render(el, ctx) {
    const range = await ctx.request("range", { days: 7 });
    el.innerHTML = `
      <div style="display:flex;align-items:baseline;gap:10px">
        <div class="w-hero" style="font-size:28px">${T.durationHtml(range.average)}</div>
        <div class="w-sub" style="margin:0">par jour en moyenne</div>
      </div>
      <div class="w-fill" id="days" style="margin-top:8px;display:flex;align-items:flex-end"></div>`;
    columnChart(el.querySelector("#days"), range.days.map((day) => ({
      value: day.total,
      label: T.capitalize(T.dateLabel(day.date, "weekday")).replace(".", ""),
      tip: `<div class="tooltip-title">${T.capitalize(T.dateLabel(day.date))}</div><div class="tooltip-row"><span class="grow">Temps d’écran</span><b>${T.duration(day.total)}</b></div>`,
    })), {
      width: ctx.size === "w" ? 700 : 340,
      height: ctx.size === "l" ? 250 : 96,
      axis: false,
      reference: range.average,
      label: "Temps d’écran des sept derniers jours",
    });
  },
});

T.widget({
  id: "screen-time.budgets",
  module: MODULE,
  title: "Budgets du jour",
  description: "Où en est chaque budget : temps utilisé et temps restant.",
  icon: "target",
  sizes: ["m", "l", "s"],
  refresh: 10,
  async render(el, ctx) {
    const result = await ctx.request("budgets");
    await ensureCatalog(ctx.request, result.catalogVersion);
    const budgets = result.budgets.filter((budget) => budget.enabled);

    if (!budgets.length) {
      el.innerHTML = `<div class="empty" style="padding:8px 0">${T.icon("target", 24)}Aucun budget actif<button class="btn" type="button" style="margin-top:4px" id="create">Créer un budget</button></div>`;
      el.querySelector("#create").addEventListener("click", () => ctx.open("screen-time/budgets"));
      return;
    }

    if (ctx.size === "s") {
      const worst = budgets.slice().sort((a, b) => b.used / b.limit - a.used / a.limit)[0];
      const remaining = Math.max(0, worst.limit - worst.used);
      el.innerHTML = `
        <div style="display:flex;align-items:center;gap:8px">${appIcon(worst.app)}<span style="font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${T.escape(appName(worst.app))}</span></div>
        <div class="w-hero" style="font-size:30px;margin-top:12px">${T.durationHtml(worst.state === "over" ? worst.used - worst.limit : remaining)}</div>
        <div class="w-sub"><span class="state ${worst.state}">${worst.state === "over" ? "de dépassement" : "restant"}</span></div>
        <div class="w-foot">${budgetMeter(worst)}</div>`;
      return;
    }

    const limit = ctx.size === "l" ? 6 : 2;
    el.innerHTML = `<div style="display:grid;gap:12px">${budgets.slice(0, limit).map((budget) => `
      <div>
        <div style="display:flex;align-items:center;gap:8px;margin-bottom:6px;font-size:12.5px">
          ${appIcon(budget.app)}<span style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:550">${T.escape(appName(budget.app))}</span>
          <span class="num" style="color:var(--text-2)">${T.duration(budget.used)} / ${T.duration(budget.limit)}</span>
        </div>
        ${budgetMeter(budget)}
        <div class="state ${budget.state}" style="margin-top:4px">${stateLabel(budget.state)}</div>
      </div>`).join("")}</div>
      ${budgets.length > limit ? `<div class="w-foot">et ${budgets.length - limit}${NB}autre${budgets.length - limit > 1 ? "s" : ""}</div>` : ""}`;
  },
});
