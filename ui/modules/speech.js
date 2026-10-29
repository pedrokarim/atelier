/*
  Module « Synthèse vocale », moitié page : l'état du raccourci, la lecture
  d'un texte, et les réglages. La moitié Rust (`src/modules/speech`) détient
  le raccourci global, lit le presse-papiers et ouvre Edge.
*/

const MODULE = "speech";
const T = Atelier;
const NB = T.NBSP;

/* --- Styles propres au module ------------------------------------------ */

const style = document.createElement("style");
style.textContent = `
.sp-keys { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; min-width: 0; }
.sp-keys kbd { display: inline-grid; place-items: center; min-width: 26px; height: 26px; padding: 0 8px; border: 1px solid var(--border-strong); border-bottom-width: 2px; border-radius: var(--radius-sm); background: var(--surface-2); color: var(--text); font: 600 12px var(--mono); }
.sp-keys .plus { color: var(--muted); font-size: 11px; }
.sp-keys.big kbd { min-width: 34px; height: 34px; padding: 0 11px; font-size: 14px; }
.sp-keys.none { color: var(--muted); font-size: 12.5px; }

.sp-status { display: flex; flex-wrap: wrap; align-items: center; gap: 12px 16px; }
.sp-status .sp-keys { flex: 0 1 auto; }
.sp-error { margin-top: 10px; color: var(--critical-text); font-size: 12px; }

/* Bouton de capture du raccourci. */
.sp-hotkey { display: flex; align-items: center; gap: 10px; width: 100%; min-height: 40px; padding: 6px 11px; border: 1px solid var(--border-strong); border-radius: var(--radius); background: var(--surface); color: var(--text-2); font: inherit; text-align: left; cursor: pointer; }
.sp-hotkey .sp-keys { flex: 1; }
.sp-hotkey:hover { border-color: var(--accent); }
.sp-hotkey:focus-visible { outline: none; border-color: var(--accent); box-shadow: var(--ring); }
.sp-hotkey.capturing { border-color: var(--accent); box-shadow: var(--ring); }
.sp-hotkey.invalid { border-color: var(--critical); }
.sp-hotkey .sp-prompt { flex: 1; min-width: 0; color: var(--accent); font-size: 12.5px; }

.sp-with-button { display: flex; gap: 8px; min-width: 0; }
.sp-with-button .input { flex: 1; min-width: 0; }
.sp-with-button .btn { flex: none; }

.sp-textarea { height: auto; min-height: 110px; padding: 9px 11px; line-height: 1.5; resize: vertical; }
.sp-count { color: var(--muted); font-size: 11.5px; font-variant-numeric: tabular-nums; }
.sp-row-end { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; margin-top: 10px; }
.sp-row-end .sp-count { margin-right: auto; }

.sp-facts { display: grid; gap: 6px; margin: 10px 0 0; padding: 0; list-style: none; color: var(--text-2); font-size: 12.5px; }
.sp-facts li { display: grid; grid-template-columns: 16px minmax(0, 1fr); gap: 8px; align-items: start; }
.sp-facts li > span { min-width: 0; overflow-wrap: anywhere; }
.sp-facts svg { margin-top: 2px; color: var(--muted); }

.sp-url { margin-top: 10px; padding: 9px 11px; border-radius: var(--radius); background: var(--surface-2); font: 11px var(--mono); color: var(--text-2); word-break: break-all; user-select: text; max-height: 160px; overflow-y: auto; }

.sp-apps { display: grid; gap: 6px; }
.sp-app { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 10px; align-items: center; width: 100%; padding: 9px 11px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); color: var(--text); font: inherit; text-align: left; cursor: pointer; }
.sp-app:hover { border-color: var(--accent); }
.sp-app-name { min-width: 0; overflow: hidden; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
.sp-app-id { min-width: 0; overflow: hidden; color: var(--muted); font: 11px var(--mono); text-overflow: ellipsis; white-space: nowrap; }

/* Widget. */
.sp-w { display: flex; flex-direction: column; height: 100%; min-width: 0; }
.sp-w-keys { min-width: 0; margin-top: 2px; }
.sp-w-keys .sp-keys { flex-wrap: nowrap; gap: 3px; }
.sp-w-keys .sp-keys kbd { min-width: 22px; height: 22px; padding: 0 6px; font-size: 11px; }
.sp-w .w-sub { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sp-w .w-sub.over { color: var(--critical-text); }
.sp-w-action { margin-top: auto; padding-top: 8px; }
.sp-w-action .btn { width: 100%; min-width: 0; justify-content: center; }
`;
document.head.appendChild(style);

/* --- Mise en forme ----------------------------------------------------- */

const integer = new Intl.NumberFormat("fr-FR", { maximumFractionDigits: 0 });

const LAUNCH_MODES = [
  { value: "app", title: "Fenêtre applicative Edge", desc: "Fonctionne sans rien installer. Une fenêtre par lecture." },
  { value: "pwa", title: "PWA installée", desc: "Réutilise la fenêtre déjà ouverte. Demande l’identifiant de l’application." },
  { value: "tab", title: "Onglet ordinaire", desc: "Ouvre un onglet dans la fenêtre Edge courante." },
];

const SOURCES = {
  hotkey: "par le raccourci",
  page: "depuis Atelier",
  text: "texte saisi",
};

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

function keysHtml(spec, big = false) {
  if (!spec) return `<span class="sp-keys none">Aucun raccourci</span>`;
  const keys = spec.split("+").map((part) => `<kbd>${T.escape(part)}</kbd>`).join('<span class="plus">+</span>');
  return `<span class="sp-keys${big ? " big" : ""}">${keys}</span>`;
}

function hotkeyState(hotkey) {
  if (hotkey.suspended) return `<span class="badge">Suspendu le temps de la saisie</span>`;
  if (hotkey.refused && !hotkey.active) return `<span class="state over">${T.icon("alert", 13)}Refusé par Windows</span>`;
  if (hotkey.refused) return `<span class="state soon">${T.icon("alert", 13)}Nouveau raccourci refusé, l’ancien reste actif</span>`;
  if (hotkey.active) return `<span class="badge live">Actif partout</span>`;
  return `<span class="badge">Désactivé</span>`;
}

function lastReadText(last) {
  if (!last) return "Aucune lecture depuis le lancement d’Atelier.";
  const time = new Date(last.at).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" });
  if (last.error) return `${time}${T.sep}${last.error}`;
  const chars = `${integer.format(last.chars)}${NB}caractère${last.chars > 1 ? "s" : ""}`;
  return `${time}${T.sep}${chars}, ${SOURCES[last.source] || ""}${last.truncated ? " (tronqué)" : ""}`;
}

/* « 1,2 » pour 1.2 : la virgule décimale, usage français. */
const decimalText = (value) => (value == null ? "" : String(value).replace(".", ","));

async function readClipboard(request, config) {
  try {
    const read = await request("speak", config ? { config } : {});
    T.toast(`${integer.format(read.chars)}${NB}caractère${read.chars > 1 ? "s" : ""} envoyé${read.chars > 1 ? "s" : ""} à la synthèse vocale${read.truncated ? ", texte tronqué" : ""}.`, "success");
  } catch (error) {
    T.toast(error.message, "error");
  }
}

/* --- Page : Lecture ----------------------------------------------------- */

function legacyCard(legacy) {
  const facts = [];
  if (legacy.running) facts.push(["power", `En cours d’exécution${legacy.running > 1 ? ` (${legacy.running}${NB}processus)` : ""}${NB}: il détient le raccourci.`]);
  if (legacy.installed) facts.push(["folder", `Installé dans ${T.escape(legacy.installed)}`]);
  if (legacy.autostart) facts.push(["clock", "Lancé à chaque ouverture de session."]);
  if (legacy.shortcut) facts.push(["file", "Présent dans le menu Démarrer."]);

  return `
    <section class="card" id="legacy">
      <div class="card-head"><div class="card-title">L’ancien programme tts-hotkey est encore là</div></div>
      <div class="field-hint" style="margin-top:0">Atelier fait désormais la même chose. Tant que l’ancien programme tourne, Windows lui réserve le raccourci et refuse de le donner à Atelier.</div>
      <ul class="sp-facts">${facts.map(([icon, text]) => `<li>${T.icon(icon, 14)}<span>${text}</span></li>`).join("")}</ul>
      <div class="field-hint">Ses réglages ont été repris. Ils restent, avec son journal, dans <span style="font-family:var(--mono);white-space:nowrap">%APPDATA%\\just-tools</span>.</div>
      <div class="card-actions"><button class="btn btn-danger" id="removeLegacy" type="button">${T.icon("trash", 15)}Retirer tts-hotkey</button></div>
    </section>`;
}

async function renderRead(root, ctx) {
  let state = await ctx.request("state");

  function render() {
    const { hotkey, legacy } = state;
    const legacyPresent = legacy.running || legacy.installed || legacy.autostart || legacy.shortcut;

    root.innerHTML = pageHead(
      "Synthèse vocale",
      "Le raccourci envoie le contenu du presse-papiers vers la synthèse vocale de Just Tools, qui démarre la lecture toute seule.",
      `<button class="btn btn-primary" id="speakClipboard" type="button">${T.icon("clipboard", 15)}Lire le presse-papiers</button>`,
    ) + `
      <div class="stack" style="max-width:760px">
        ${legacyPresent ? legacyCard(legacy) : ""}

        <section class="card">
          <div class="card-head"><div class="card-title">Raccourci</div></div>
          <div class="sp-status">
            ${keysHtml(hotkey.active || hotkey.refused, true)}
            ${hotkeyState(hotkey)}
          </div>
          ${hotkey.error ? `<div class="sp-error">${T.escape(hotkey.error)}.</div>` : ""}
          <div class="field-hint">Copiez un texte n’importe où, puis appuyez sur le raccourci${NB}: Edge s’ouvre et lit.</div>
          <div class="card-actions"><button class="btn" id="changeHotkey" type="button">${T.icon("keyboard", 15)}Changer le raccourci</button></div>
        </section>

        <section class="card">
          <div class="card-head"><div class="card-title">Lire un texte</div></div>
          <textarea class="input sp-textarea" id="text" spellcheck="true" placeholder="Saisissez ou collez le texte à lire"></textarea>
          <div class="sp-row-end">
            <span class="sp-count" id="count"></span>
            <button class="btn" id="speakText" type="button">${T.icon("play", 15)}Lire ce texte</button>
          </div>
        </section>

        <section class="card">
          <div class="card-head"><div class="card-title">Dernière lecture</div></div>
          <div class="field-hint" style="margin-top:0" id="last">${T.escape(lastReadText(state.last))}</div>
        </section>
      </div>`;

    const text = root.querySelector("#text");
    const count = root.querySelector("#count");
    const updateCount = () => {
      const length = [...text.value].length;
      const limit = state.config.max_chars;
      count.textContent = length
        ? `${integer.format(length)}${NB}caractère${length > 1 ? "s" : ""}${limit && length > limit ? `, tronqué à ${integer.format(limit)}` : ""}`
        : "";
    };
    text.addEventListener("input", updateCount);
    updateCount();

    root.querySelector("#speakClipboard").addEventListener("click", () => readClipboard(ctx.request));
    root.querySelector("#changeHotkey").addEventListener("click", () => ctx.navigate("settings"));
    root.querySelector("#speakText").addEventListener("click", async () => {
      if (!text.value.trim()) {
        T.toast("Le texte est vide.", "error");
        text.focus();
        return;
      }
      try {
        await ctx.request("speak", { text: text.value });
      } catch (error) {
        T.toast(error.message, "error");
      }
    });

    const remove = root.querySelector("#removeLegacy");
    if (remove) {
      remove.addEventListener("click", async () => {
        const ok = await T.confirm(
          `Retirer tts-hotkey${NB}?`,
          "Le programme est arrêté, puis son installation, son raccourci du menu Démarrer et son démarrage automatique sont supprimés. Ses réglages et son journal restent en place.",
          "Retirer",
        );
        if (!ok) return;
        remove.disabled = true;
        try {
          const result = await ctx.request("legacy.remove");
          state = { ...state, ...result };
          T.toast(result.hotkey.active
            ? `tts-hotkey est retiré. Le raccourci ${result.hotkey.active} appartient désormais à Atelier.`
            : "tts-hotkey est retiré.", "success");
        } catch (error) {
          T.toast(error.message, "error");
          state = await ctx.request("state");
        }
        render();
      });
    }
  }

  render();

  ctx.on("speech.read", (last) => {
    state.last = last;
    const node = root.querySelector("#last");
    if (node) node.textContent = lastReadText(last);
  });
}

/* --- Page : Réglages ---------------------------------------------------- */

const HOTKEY_HINT = "Cliquez, puis composez la combinaison. Au moins un modificateur est requis (Ctrl, Alt, Shift ou Win).";

/* Traduit `KeyboardEvent.code` vers les noms que la moitié Rust sait relire. */
function keyName(event) {
  const code = event.code || "";
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  if (/^Numpad[0-9]$/.test(code)) return "Num" + code.slice(6);
  const named = {
    Space: "Space", Enter: "Enter", NumpadEnter: "Enter", Tab: "Tab",
    Backspace: "Backspace", Escape: "Escape", Insert: "Insert", Delete: "Delete",
    Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown",
    ArrowLeft: "Left", ArrowUp: "Up", ArrowRight: "Right", ArrowDown: "Down",
    Pause: "Pause", PrintScreen: "PrintScreen",
  };
  return named[code] || null;
}

function toggle(id, title, desc, checked) {
  return `
    <label class="toggle">
      <input type="checkbox" id="${id}" ${checked ? "checked" : ""} />
      <span class="track"><span class="thumb"></span></span>
      <span class="toggle-text">
        <span class="toggle-title">${title}</span>
        <span class="toggle-desc">${desc}</span>
      </span>
    </label>`;
}

function text(id, label, value, { placeholder = "", hint = "", mode = "" } = {}) {
  return `
    <div class="field">
      <label class="field-label" for="${id}">${label}</label>
      <input class="input" id="${id}" spellcheck="false" ${mode ? `inputmode="${mode}"` : ""} placeholder="${T.escape(placeholder)}" value="${T.escape(value ?? "")}" />
      ${hint ? `<div class="field-hint">${hint}</div>` : ""}
    </div>`;
}

async function renderSettings(root, ctx) {
  const state = await ctx.request("state");
  const config = state.config;
  let hotkey = config.hotkey;
  let previous = hotkey;
  let capturing = false;

  root.innerHTML = pageHead(
    "Réglages de la lecture",
    "Le raccourci, la façon d’ouvrir l’outil, et ce qui est transmis à la page de synthèse vocale.",
    `<button class="btn" id="test" type="button">${T.icon("play", 15)}Tester</button>
     <button class="btn btn-primary" id="save" type="button">${T.icon("check", 15)}Enregistrer</button>`,
  ) + `
    <div class="stack" style="max-width:760px">
      <section class="card">
        <div class="card-head"><div class="card-title">Raccourci et ouverture</div></div>
        <div class="field">
          <label class="field-label" for="hotkey">Raccourci global</label>
          <button class="sp-hotkey" id="hotkey" type="button"></button>
          <div class="field-hint" id="hotkeyHint">${HOTKEY_HINT}</div>
        </div>
        ${text("baseUrl", "Adresse du site", config.base_url, { hint: "La racine du site, ou l’adresse complète de l’outil de synthèse vocale." })}
        <div class="field">
          <span class="field-label">Mode d’ouverture</span>
          <div class="choices">${LAUNCH_MODES.map((mode) => `
            <label class="choice">
              <input type="radio" name="launchMode" value="${mode.value}" ${config.launch_mode === mode.value ? "checked" : ""} />
              <span class="choice-dot"></span>
              <span class="choice-body">
                <span class="choice-title">${mode.title}</span>
                <span class="choice-desc">${mode.desc}</span>
              </span>
            </label>`).join("")}
          </div>
        </div>
        <div class="field">
          <label class="field-label" for="appId">Identifiant de la PWA</label>
          <div class="sp-with-button">
            <input class="input" id="appId" spellcheck="false" placeholder="Aucune" value="${T.escape(config.app_id || "")}" />
            <button class="btn" id="chooseApp" type="button">${T.icon("search", 15)}Choisir…</button>
          </div>
          <div class="field-hint">Requis par le mode «${NB}PWA installée${NB}». Edge l’affiche aussi dans edge://apps.</div>
        </div>
        <div class="grid-2">
          ${text("profile", "Profil Edge", config.profile_directory, { placeholder: "Default" })}
          <div class="field">
            <label class="field-label" for="edgePath">Chemin du navigateur</label>
            <div class="sp-with-button">
              <input class="input" id="edgePath" spellcheck="false" placeholder="Détection automatique" value="${T.escape(config.edge_path || "")}" title="${T.escape(config.edge_path || state.edge || "")}" />
              <button class="btn" id="browse" type="button">${T.icon("folder", 15)}Parcourir</button>
            </div>
            <div class="field-hint">${state.edge ? `Détecté${NB}: <span class="path" style="display:inline">${T.escape(state.edge)}</span>` : "Edge n’a pas été trouvé à son emplacement habituel."}</div>
          </div>
        </div>
      </section>

      <section class="card">
        <div class="card-head"><div class="card-title">Lecture</div></div>
        <div class="field-hint" style="margin-top:0">Transmis à la page de synthèse vocale. Les champs laissés vides gardent les valeurs de la voix du système.</div>
        ${toggle("autoplay", "Démarrer la lecture dès l’ouverture", "Sans cela, il faut appuyer sur Lecture dans la page.", config.autoplay)}
        ${toggle("readerMode", `Interface «${NB}lecteur${NB}» épurée`, "Masque le reste de l’outil pendant l’écoute.", config.reader_mode)}
        <div class="grid-3" style="margin-top:14px">
          ${text("rate", "Débit", decimalText(config.rate), { placeholder: "1,0", mode: "decimal", hint: "De 0,1 à 10." })}
          ${text("pitch", "Hauteur", decimalText(config.pitch), { placeholder: "1,0", mode: "decimal", hint: "De 0 à 2." })}
          ${text("volume", "Volume", decimalText(config.volume), { placeholder: "1,0", mode: "decimal", hint: "De 0 à 1." })}
        </div>
        <div class="grid-2">
          ${text("voice", "Voix", config.voice, { placeholder: "Voix par défaut", hint: `Nom exact ou partiel${NB}: «${NB}Denise${NB}», «${NB}Natural${NB}»…` })}
          ${text("lang", "Langue", config.lang, { placeholder: "fr-FR" })}
        </div>
      </section>

      <section class="card">
        <div class="card-head"><div class="card-title">Traitement du texte</div></div>
        ${text("maxChars", "Longueur maximale", String(config.max_chars), { mode: "numeric", hint: `En caractères, jusqu’à ${integer.format(state.maxCharsLimit)}. Au-delà, le texte est tronqué${NB}: la ligne de commande de Windows est limitée à 32${NB}767${NB}caractères.` })}
        ${toggle("collapse", "Aplatir les espaces et les retours à la ligne", "Utile après une copie depuis un PDF ou une reconnaissance de texte.", config.collapse_whitespace)}
        ${toggle("dehyphenate", "Recoller les mots coupés en fin de ligne", `«${NB}exem-⏎ple${NB}» devient «${NB}exemple${NB}».`, config.dehyphenate)}
      </section>

      <section class="card">
        <div class="card-head"><div class="card-title">Déblocage de la lecture automatique</div></div>
        <div class="field-hint" style="margin-top:0">Chromium refuse de parler tant que la page n’a reçu aucune interaction. Après l’ouverture, Atelier met la fenêtre au premier plan et envoie une frappe F24, que le navigateur compte comme un geste de l’utilisateur.</div>
        ${toggle("focusWindow", "Mettre la fenêtre au premier plan", "Nécessaire pour que la frappe atteigne la page.", config.focus_window)}
        ${toggle("activationKey", "Envoyer la frappe d’activation (F24)", `F24 n’existe sur aucun clavier courant${NB}: aucun effet de bord dans la page.`, config.send_activation_key)}
        ${toggle("beep", "Bip à chaque déclenchement", "Confirme que le raccourci a bien été reçu.", config.beep)}
        <div class="grid-2" style="margin-top:14px">
          ${text("delay", "Délai avant activation", String(config.activation_delay_ms), { mode: "numeric", hint: "En millisecondes, le temps que la fenêtre s’ouvre." })}
          ${text("titleMatch", "Titre de la fenêtre", config.window_title_match, { hint: "Un fragment du titre de la page de l’outil." })}
        </div>
      </section>

      <section class="card">
        <div class="card-head"><div class="card-title">Diagnostic</div></div>
        <div class="field-hint" style="margin-top:0">L’adresse qui serait ouverte pour le contenu actuel du presse-papiers, avec les réglages de cette page, sans rien lancer.</div>
        <div id="preview"></div>
        <div class="card-actions">
          <button class="btn" id="showPreview" type="button">${T.icon("eye", 15)}Afficher l’adresse</button>
          <button class="btn" id="openData" type="button">${T.icon("folder", 15)}Ouvrir le dossier du module</button>
        </div>
      </section>
    </div>`;

  const $ = (id) => root.querySelector(`#${id}`);
  const button = $("hotkey");
  const hint = $("hotkeyHint");

  /* --- Capture du raccourci -------------------------------------------- */

  function renderHotkey() {
    button.innerHTML = capturing
      ? `<span class="sp-prompt">Composez la combinaison…</span>${T.icon("keyboard", 16)}`
      : `${keysHtml(hotkey)}${T.icon("keyboard", 16)}`;
  }

  /*
    La capture démarre sur une action délibérée, jamais sur le simple focus.
    Le raccourci global est suspendu pendant ce temps : sans cela, composer
    la combinaison actuelle lancerait une lecture au lieu d'être saisie.
  */
  function startCapture() {
    if (capturing) return;
    capturing = true;
    previous = hotkey;
    button.classList.add("capturing");
    button.classList.remove("invalid");
    hint.textContent = "Composez la combinaison. Échap annule, Suppr retire le raccourci.";
    renderHotkey();
    ctx.request("hotkey.suspend").catch(() => {});
  }

  function stopCapture(restore) {
    if (!capturing) return;
    capturing = false;
    if (restore) hotkey = previous;
    button.classList.remove("capturing");
    hint.textContent = HOTKEY_HINT;
    renderHotkey();
    ctx.request("hotkey.resume").catch(() => {});
  }

  button.addEventListener("click", startCapture);
  button.addEventListener("blur", () => stopCapture(false));
  ctx.onCleanup(() => stopCapture(true));

  button.addEventListener("keydown", (event) => {
    if (!capturing) return;
    event.preventDefault();
    event.stopPropagation();

    // Échap rend la combinaison précédente : une capture lancée par erreur
    // ne doit rien coûter.
    if (event.key === "Escape") {
      stopCapture(true);
      button.blur();
      return;
    }
    // Un modificateur seul ne forme pas encore un raccourci.
    if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) return;

    if ((event.key === "Delete" || event.key === "Backspace")
        && !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey) {
      hotkey = "";
      stopCapture(false);
      button.blur();
      return;
    }

    const name = keyName(event);
    if (!name) return;

    const parts = [];
    if (event.ctrlKey) parts.push("Ctrl");
    if (event.altKey) parts.push("Alt");
    if (event.shiftKey) parts.push("Shift");
    if (event.metaKey) parts.push("Win");

    if (!parts.length) {
      button.classList.add("invalid");
      hint.textContent = `Ce raccourci serait actif partout, même en écrivant${NB}: ajoutez Ctrl, Alt, Shift ou Win.`;
      return;
    }

    parts.push(name);
    hotkey = parts.join("+");
    button.classList.remove("invalid");
    stopCapture(false);
    button.blur();
  });

  renderHotkey();

  /* --- Lecture des champs ---------------------------------------------- */

  const value = (id) => $(id).value.trim();
  const optional = (id) => (value(id) === "" ? null : value(id));

  function mark(id, invalid) {
    $(id).classList.toggle("invalid", invalid);
  }

  function decimal(id, label, errors) {
    const raw = value(id).replace(",", ".");
    if (raw === "") { mark(id, false); return null; }
    const parsed = Number(raw);
    const invalid = !Number.isFinite(parsed);
    mark(id, invalid);
    if (invalid) errors.push(`${label}${NB}: «${NB}${value(id)}${NB}» n’est pas un nombre.`);
    return invalid ? null : parsed;
  }

  function whole(id, label, errors) {
    const invalid = !/^\d+$/.test(value(id));
    mark(id, invalid);
    if (invalid) errors.push(`${label} doit être un nombre entier.`);
    return invalid ? 0 : Number(value(id));
  }

  function collect() {
    const errors = [];
    const mode = root.querySelector('input[name="launchMode"]:checked');

    mark("baseUrl", value("baseUrl") === "");
    if (value("baseUrl") === "") errors.push("L’adresse du site ne peut pas être vide.");

    const pwaWithoutId = mode && mode.value === "pwa" && optional("appId") === null;
    mark("appId", pwaWithoutId);
    if (pwaWithoutId) errors.push(`Le mode «${NB}PWA installée${NB}» demande l’identifiant de l’application.`);

    const next = {
      base_url: value("baseUrl"),
      hotkey,
      launch_mode: mode ? mode.value : "app",
      edge_path: optional("edgePath"),
      app_id: optional("appId"),
      profile_directory: optional("profile"),
      autoplay: $("autoplay").checked,
      reader_mode: $("readerMode").checked,
      rate: decimal("rate", "Débit", errors),
      pitch: decimal("pitch", "Hauteur", errors),
      volume: decimal("volume", "Volume", errors),
      voice: optional("voice"),
      lang: optional("lang"),
      max_chars: whole("maxChars", "La longueur maximale", errors),
      collapse_whitespace: $("collapse").checked,
      dehyphenate: $("dehyphenate").checked,
      focus_window: $("focusWindow").checked,
      send_activation_key: $("activationKey").checked,
      activation_delay_ms: whole("delay", "Le délai avant activation", errors),
      window_title_match: value("titleMatch"),
      beep: $("beep").checked,
    };

    if (errors.length) {
      T.toast(errors.join("\n"), "error");
      return null;
    }
    return next;
  }

  /* --- Actions ----------------------------------------------------------- */

  $("save").addEventListener("click", async () => {
    const next = collect();
    if (!next) return;
    try {
      const saved = await ctx.request("settings.save", { config: next });
      hotkey = saved.config.hotkey;
      renderHotkey();
      if (saved.warning) {
        button.classList.add("invalid");
        T.toast(`Réglages enregistrés, sauf le raccourci${NB}: ${saved.warning}`, "error");
      } else {
        T.toast("Réglages enregistrés.", "success");
      }
    } catch (error) {
      T.toast(error.message, "error");
    }
  });

  // Les réglages en cours d'édition, sans les enregistrer : c'est tout
  // l'intérêt d'un essai.
  $("test").addEventListener("click", () => {
    const next = collect();
    if (next) readClipboard(ctx.request, next);
  });

  $("showPreview").addEventListener("click", async () => {
    const next = collect();
    if (!next) return;
    try {
      const preview = await ctx.request("preview", { config: next });
      $("preview").innerHTML = `
        <div class="sp-url">${T.escape(preview.url)}</div>
        <div class="field-hint">${integer.format(preview.chars)}${NB}caractères lus${T.sep}${integer.format(preview.length)}${NB}caractères d’adresse</div>`;
    } catch (error) {
      T.toast(error.message, "error");
    }
  });

  $("browse").addEventListener("click", async () => {
    try {
      const path = await ctx.request("browse");
      if (path) {
        $("edgePath").value = path;
        $("edgePath").title = path;
      }
    } catch (error) {
      T.toast(error.message, "error");
    }
  });

  $("chooseApp").addEventListener("click", async () => {
    const apps = await ctx.request("apps").catch(() => []);
    if (!apps.length) {
      T.toast("Aucune application installée dans Edge. Ouvrez le site dans Edge, puis menu « … » → Applications → « Installer ce site en tant qu’application ».", "error");
      return;
    }
    const ranked = [...apps].sort((a, b) =>
      Number(/just tools/i.test(b.name)) - Number(/just tools/i.test(a.name)) || a.name.localeCompare(b.name, "fr"));
    T.dialog({
      title: "Applications installées dans Edge",
      subtitle: "Celle de Just Tools est proposée en premier.",
      build(body, close) {
        body.innerHTML = `<div class="sp-apps">${ranked.map((app, index) => `
          <button class="sp-app" type="button" data-index="${index}">
            <span style="min-width:0">
              <span class="sp-app-name" title="${T.escape(app.name)}" style="display:block">${T.escape(app.name)}</span>
              <span class="sp-app-id" title="${T.escape(app.id)}" style="display:block">${T.escape(app.id)}</span>
            </span>
            <span class="badge">${T.escape(app.profile)}</span>
          </button>`).join("")}</div>`;
        body.querySelectorAll(".sp-app").forEach((node) => node.addEventListener("click", () => {
          const app = ranked[Number(node.dataset.index)];
          $("appId").value = app.id;
          $("profile").value = app.profile;
          mark("appId", false);
          close();
        }));
      },
      actions: [{ label: "Annuler", ghost: true }],
    });
  });

  $("openData").addEventListener("click", () => ctx.request("open.data").catch((error) => T.toast(error.message, "error")));
}

/* --- Déclaration du module ---------------------------------------------- */

T.module({
  id: MODULE,
  name: "Synthèse vocale",
  icon: "speaker",
  description: "Un raccourci global qui fait lire le presse-papiers par la synthèse vocale de Just Tools.",
  pages: [
    { id: "read", title: "Lecture", icon: "speaker", render: renderRead },
    { id: "settings", title: "Réglages", icon: "sliders", render: renderSettings },
  ],
});

/* --- Widget -------------------------------------------------------------- */

T.widget({
  id: "speech.reader",
  module: MODULE,
  title: "Synthèse vocale",
  description: "Le raccourci qui fait lire le presse-papiers, et un bouton pour lancer la lecture.",
  icon: "speaker",
  sizes: ["s", "m"],
  refresh: 15,
  events: ["speech.read"],
  async render(el, ctx) {
    const state = await ctx.request("state");
    const { hotkey } = state;
    const spec = hotkey.active || hotkey.refused;
    const refused = Boolean(hotkey.refused);
    const last = lastReadText(state.last);
    const sub = !spec ? "Aucun raccourci"
      : refused ? (hotkey.active ? "Nouveau raccourci refusé" : "Refusé par Windows")
      : ctx.size === "s" ? "Lit le presse-papiers" : last;
    const small = ctx.size === "s";

    el.innerHTML = `
      <div class="sp-w">
        <div class="sp-w-keys" title="${T.escape(spec || "")}">${keysHtml(spec)}</div>
        <div class="w-sub${refused ? " over" : ""}" title="${T.escape(refused ? hotkey.error || sub : last)}">${T.escape(sub)}</div>
        <div class="sp-w-action"><button class="btn" type="button" title="Lire le presse-papiers">${T.icon("clipboard", 15)}${small ? "Lire" : "Lire le presse-papiers"}</button></div>
      </div>`;
    el.querySelector(".sp-w-action .btn").addEventListener("click", (event) => {
      event.stopPropagation();
      readClipboard(ctx.request);
    });
  },
});
