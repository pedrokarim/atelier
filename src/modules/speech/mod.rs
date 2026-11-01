//! Module « Synthèse vocale » : un raccourci global qui envoie le
//! presse-papiers vers la synthèse vocale de Just Tools.
//!
//! ```text
//! Ctrl+Alt+S → presse-papiers → base64url → Edge → /tools/text-to-speech?b64=…&autoplay=1
//! ```
//!
//! Un second raccourci fait le même trajet, le texte traduit en chemin (voir
//! `translate`). La PWA installée, si elle existe, reçoit le texte dans sa
//! fenêtre déjà ouverte ; sinon Edge ouvre une fenêtre par lecture.
//!
//! Chromium refuse `speechSynthesis.speak()` sur un document qui n'a reçu
//! aucune interaction. Après le lancement, le module met donc la fenêtre au
//! premier plan et envoie une frappe F24 : une vraie frappe au niveau du
//! système, que le navigateur compte comme geste utilisateur.
//!
//! Il remplace l'ancien programme autonome `tts-hotkey`, dont il reprend les
//! réglages et qu'il sait retirer (voir `legacy`).

mod clipboard;
mod config;
mod hotkey;
mod launcher;
mod legacy;
mod translate;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result, anyhow};
use serde::Serialize;
use serde_json::{Value, json};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OPENFILENAMEW};
use windows::Win32::UI::WindowsAndMessaging::MB_OK;
use windows::core::{PCWSTR, PWSTR};

use self::config::{Config, LaunchMode};
use self::hotkey::Hotkey;
use self::launcher::InstalledApp;
use super::{Module, ModuleContext};
use crate::ipc::{HostSender, Notice, NoticeAction, Responder};
use crate::logging::log_line;

pub const ID: &str = "speech";

/// Page ouverte par les notifications du module.
const MAIN_PAGE: &str = "speech/read";

fn data_dir() -> Result<PathBuf> {
    crate::paths::module_dir(ID)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Message d'erreur en phrase : majuscule initiale, point final.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

/// Répond à une requête, l'erreur mise en phrase : la page l'affiche telle
/// quelle.
fn respond<T: Serialize>(reply: Responder, result: Result<T>) {
    match result {
        Ok(value) => reply.ok(value),
        Err(error) => reply.err(sentence(&format!("{error:#}"))),
    }
}

/// D'où vient le texte lu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Source {
    Hotkey,
    Page,
    Text,
}

/// Ce qu'il faut faire du texte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Read,
    TranslateThenRead,
}

/// La dernière lecture, montrée par la page et le widget.
#[derive(Debug, Clone, Serialize)]
struct LastRead {
    at: i64,
    source: Source,
    chars: usize,
    truncated: bool,
    /// Langue d'origine, quand le texte a été traduit.
    translated_from: Option<String>,
    error: Option<String>,
}

struct Shared {
    config: Mutex<Config>,
    last: Mutex<Option<LastRead>>,
    host: Mutex<Option<HostSender>>,
}

impl Shared {
    fn host(&self) -> Option<HostSender> {
        self.host.lock().unwrap().clone()
    }

    fn notify(
        &self,
        id: &str,
        title: &str,
        body: String,
        tone: &'static str,
        timeout: Option<u32>,
    ) {
        let Some(host) = self.host() else { return };
        host.notify(Notice {
            id: format!("{ID}:{id}"),
            module: ID.to_string(),
            title: title.to_string(),
            body,
            icon: None,
            glyph: Some("speaker"),
            tone,
            countdown: None,
            timeout,
            actions: vec![NoticeAction::new("open", "Ouvrir la synthèse vocale", true)],
        });
    }

    fn record(&self, read: LastRead) {
        *self.last.lock().unwrap() = Some(read.clone());
        if let Some(host) = self.host() {
            host.broadcast("speech.read", json!(read));
        }
    }
}

/// Coupe un texte à `limit` caractères (0 : aucune limite) et dit s'il l'a
/// fait.
fn bounded(text: String, limit: usize) -> (String, bool) {
    if limit > 0 && text.chars().count() > limit {
        (text.chars().take(limit).collect(), true)
    } else {
        (text, false)
    }
}

/// Envoie un texte vers l'outil, traduit au passage si `action` le demande.
/// La lecture automatique reste à débloquer ensuite, hors du fil de
/// l'interface (`launcher::activate`).
fn speak(
    shared: &Shared,
    config: &Config,
    text: &str,
    source: Source,
    action: Action,
) -> Result<LastRead> {
    let unbounded = Config {
        max_chars: 0,
        ..config.clone()
    }
    .prepare_text(text);
    if unbounded.is_empty() {
        return Err(anyhow!("rien à lire\u{a0}: le texte est vide"));
    }
    let (prepared, mut truncated) = bounded(unbounded, config.max_chars);

    let (spoken, translated_from, reading) = match action {
        Action::Read => (prepared, None, config.clone()),
        Action::TranslateThenRead => {
            let language = &config.translate_target;
            let translation = translate::translate(&prepared, language)?;
            // Une traduction peut être plus longue que l'original.
            let (spoken, cut) = bounded(translation.text, config.max_chars);
            truncated |= cut;
            (spoken, translation.source, config.speaking(language))
        }
    };

    let target = launcher::build_url(&reading, &spoken);
    let chars = spoken.chars().count();
    log_line!(
        "synthèse vocale : {chars} caractères ({} octets d'adresse){}{}",
        target.len(),
        if truncated { ", texte tronqué" } else { "" },
        match &translated_from {
            Some(language) => format!(", traduit depuis {language}"),
            None => String::new(),
        }
    );

    launcher::launch(&reading, &target)?;

    let read = LastRead {
        at: now_ms(),
        source,
        chars,
        truncated,
        translated_from,
        error: None,
    };
    shared.record(read.clone());
    Ok(read)
}

/// Lecture du presse-papiers, avec la configuration donnée.
fn speak_clipboard(
    shared: &Shared,
    config: &Config,
    source: Source,
    action: Action,
) -> Result<LastRead> {
    if config.beep {
        unsafe {
            let _ = MessageBeep(MB_OK);
        }
    }

    let text = clipboard::read_text()?;
    if text.trim().is_empty() {
        return Err(anyhow!("le presse-papiers ne contient aucun texte"));
    }
    speak(shared, config, &text, source, action)
}

/// Appui sur un raccourci global : son fil rend la main tout de suite, la
/// lecture se fait à côté.
fn on_hotkey(shared: &Arc<Shared>, action: Action) {
    let shared = shared.clone();
    std::thread::spawn(move || {
        let config = shared.config.lock().unwrap().clone();
        match speak_clipboard(&shared, &config, Source::Hotkey, action) {
            Ok(_) => launcher::activate(&config),
            Err(error) => {
                failed(&shared, Source::Hotkey, &error);
            }
        }
    });
}

/// Une lecture qui n'a pas abouti : notée, et signalée par une notification
/// quand elle venait du raccourci, sans page pour l'afficher.
fn failed(shared: &Shared, source: Source, error: &anyhow::Error) -> String {
    let message = sentence(&format!("{error:#}"));
    log_line!("synthèse vocale : {message}");
    shared.record(LastRead {
        at: now_ms(),
        source,
        chars: 0,
        truncated: false,
        translated_from: None,
        error: Some(message.clone()),
    });
    if source == Source::Hotkey {
        shared.notify(
            "failed",
            "Rien n’a été lu",
            message.clone(),
            "warning",
            Some(6),
        );
    }
    message
}

/// Rattache les réglages à la PWA réellement installée.
///
/// Un identifiant mort est remplacé : Edge ne ferait rien de lui. Le mode
/// *PWA* n'est imposé qu'une fois, aux réglages d'avant le format 1, qui
/// n'avaient jamais pu le choisir : après cela, le choix de l'utilisateur
/// fait foi.
fn adopt_installed_app(mut config: Config, app: Option<InstalledApp>, installed: bool) -> Config {
    let Some(app) = app else { return config };

    if !installed {
        log_line!("PWA détectée : {} ({})", app.id, app.profile);
        config.app_id = Some(app.id);
        config.profile_directory = Some(app.profile);
    }
    if config.schema < config::SCHEMA && config.launch_mode == LaunchMode::App {
        log_line!("mode d'ouverture : la PWA installée remplace la fenêtre applicative");
        config.launch_mode = LaunchMode::Pwa;
    }
    config
}

/// Réglages de démarrage : ceux du module ou, au tout premier lancement,
/// ceux de l'ancien programme ou les valeurs par défaut ; rattachés dans tous
/// les cas à la PWA installée.
fn initial_config() -> Config {
    let exists = Config::exists();
    let stored = if exists {
        Config::load()
    } else {
        legacy::import_config().unwrap_or_default()
    };

    let app = launcher::detect_app(&stored.base_url);
    let installed = launcher::is_installed(&stored);
    let config = adopt_installed_app(stored.clone(), app, installed).normalized();

    if (!exists || config != stored)
        && let Err(error) = config.save()
    {
        log_line!("réglages de la synthèse vocale non enregistrés : {error:#}");
    }
    config
}

/// Sélecteur de fichier pour le chemin du navigateur.
fn browse_for_browser() -> Option<String> {
    let mut buffer = [0u16; 520];
    let filter: Vec<u16> = "Exécutables\0*.exe\0Tous les fichiers\0*.*\0\0"
        .encode_utf16()
        .collect();
    let title = crate::to_wide("Choisir le navigateur");

    let mut options = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_FILEMUSTEXIST,
        ..Default::default()
    };

    if !unsafe { GetOpenFileNameW(&mut options) }.as_bool() {
        return None;
    }

    let end = buffer.iter().position(|value| *value == 0).unwrap_or(0);
    Some(String::from_utf16_lossy(&buffer[..end]))
}

/// Remplace la combinaison d'un raccourci. Refusée, l'ancienne reste en place
/// et les réglages la reprennent ; la raison est rendue.
fn apply_hotkey(hotkey: Option<&Arc<Hotkey>>, spec: &mut String) -> Option<String> {
    let hotkey = hotkey?;
    let error = hotkey.set(spec).err()?;
    if let Some(active) = hotkey.status().active {
        *spec = active;
    }
    Some(sentence(&error))
}

pub struct Speech {
    shared: Arc<Shared>,
    hotkey: Mutex<Option<Arc<Hotkey>>>,
    translate_hotkey: Mutex<Option<Arc<Hotkey>>>,
}

impl Speech {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                config: Mutex::new(Config::default()),
                last: Mutex::new(None),
                host: Mutex::new(None),
            }),
            hotkey: Mutex::new(None),
            translate_hotkey: Mutex::new(None),
        }
    }

    fn hotkey(&self) -> Option<Arc<Hotkey>> {
        self.hotkey.lock().unwrap().clone()
    }

    fn translate_hotkey(&self) -> Option<Arc<Hotkey>> {
        self.translate_hotkey.lock().unwrap().clone()
    }

    fn hotkey_status(&self) -> hotkey::Status {
        self.hotkey()
            .map(|hotkey| hotkey.status())
            .unwrap_or_default()
    }

    fn translate_hotkey_status(&self) -> hotkey::Status {
        self.translate_hotkey()
            .map(|hotkey| hotkey.status())
            .unwrap_or_default()
    }

    /// Les deux raccourcis, le temps de suspendre ou de rétablir : la page
    /// en capture une combinaison, qui ne doit déclencher aucun des deux.
    fn hotkeys(&self) -> impl Iterator<Item = Arc<Hotkey>> {
        [self.hotkey(), self.translate_hotkey()]
            .into_iter()
            .flatten()
    }

    fn state(&self) -> Value {
        json!({
            "config": *self.shared.config.lock().unwrap(),
            "hotkey": self.hotkey_status(),
            "translateHotkey": self.translate_hotkey_status(),
            "last": *self.shared.last.lock().unwrap(),
            "legacy": legacy::traces(),
            "edge": launcher::detected_edge().map(|path| path.display().to_string()),
            "maxCharsLimit": config::MAX_CHARS_LIMIT,
        })
    }

    /// Démarre le fil d'un raccourci et lui donne sa combinaison. Un refus de
    /// Windows n'arrête rien : le raccourci reste à régler depuis la page.
    fn start_hotkey(&self, spec: &str, action: Action) -> Option<Arc<Hotkey>> {
        let shared = self.shared.clone();
        let hotkey = match Hotkey::start(move || on_hotkey(&shared, action)) {
            Ok(hotkey) => Arc::new(hotkey),
            Err(error) => {
                log_line!("raccourci de synthèse vocale indisponible : {error:#}");
                return None;
            }
        };

        if let Err(error) = hotkey.set(spec) {
            let traces = legacy::traces();
            let body = if traces.running > 0 {
                format!(
                    "{} L’ancien programme tts-hotkey tourne encore et le détient\u{a0}: retirez-le depuis la page Synthèse vocale.",
                    sentence(&error)
                )
            } else {
                sentence(&error)
            };
            let (id, title) = match action {
                Action::Read => ("hotkey", "Raccourci de synthèse vocale indisponible"),
                Action::TranslateThenRead => {
                    ("translate-hotkey", "Raccourci de traduction indisponible")
                }
            };
            self.shared.notify(id, title, body, "warning", None);
        }

        Some(hotkey)
    }

    /// Configuration envoyée par la page, validée.
    fn config_param(params: &Value) -> Result<Config> {
        let raw = params
            .get("config")
            .filter(|value| !value.is_null())
            .ok_or_else(|| anyhow!("réglages absents de la requête"))?;
        let config: Config = serde_json::from_value(raw.clone())
            .map_err(|error| anyhow!("réglages invalides\u{a0}: {error}"))?;
        let config = config.normalized();

        if config.base_url.is_empty() {
            return Err(anyhow!("l’adresse du site ne peut pas être vide"));
        }
        if !translate::is_language(&config.translate_target) {
            return Err(anyhow!(
                "«\u{a0}{}\u{a0}» n’est pas un code de langue",
                config.translate_target
            ));
        }

        let parsed = [&config.hotkey, &config.translate_hotkey].map(|spec| {
            (!spec.is_empty())
                .then(|| hotkey::parse(spec))
                .transpose()
                .map_err(|error| anyhow!("raccourci invalide\u{a0}: {error:#}"))
        });
        let [read, translation] = parsed;
        if let (Some(read), Some(translation)) = (read?, translation?)
            && (read.modifiers, read.key) == (translation.modifiers, translation.key)
        {
            return Err(anyhow!(
                "la lecture et la traduction ne peuvent pas partager le même raccourci"
            ));
        }
        Ok(config)
    }

    /// Enregistre les réglages. Un raccourci refusé par Windows n'empêche
    /// pas d'enregistrer le reste : l'ancien est conservé, et la réponse le
    /// dit.
    fn save(&self, params: &Value) -> Result<Value> {
        let mut config = Self::config_param(params)?;

        let warnings: Vec<String> = [
            apply_hotkey(self.hotkey().as_ref(), &mut config.hotkey),
            apply_hotkey(
                self.translate_hotkey().as_ref(),
                &mut config.translate_hotkey,
            ),
        ]
        .into_iter()
        .flatten()
        .collect();
        let warning = (!warnings.is_empty()).then(|| warnings.join(" "));

        config.save()?;
        *self.shared.config.lock().unwrap() = config;
        log_line!("réglages de la synthèse vocale enregistrés");

        let mut state = self.state();
        state["warning"] = json!(warning);
        Ok(state)
    }

    fn dispatch(&self, method: &str, params: Value, reply: Responder) {
        let shared = self.shared.clone();

        match method {
            "state" => reply.ok(self.state()),

            "settings.save" => respond(reply, self.save(&params)),

            // Lecture depuis la page : le presse-papiers, ou un texte saisi,
            // avec les réglages en cours d'édition s'ils sont fournis.
            "speak" => {
                let config = if params.get("config").is_some_and(|value| !value.is_null()) {
                    match Self::config_param(&params) {
                        Ok(config) => config,
                        Err(error) => return reply.err(sentence(&format!("{error:#}"))),
                    }
                } else {
                    shared.config.lock().unwrap().clone()
                };
                let text = params
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let action = if params.get("translate").and_then(Value::as_bool) == Some(true) {
                    Action::TranslateThenRead
                } else {
                    Action::Read
                };

                std::thread::spawn(move || {
                    let result = match &text {
                        Some(text) => speak(&shared, &config, text, Source::Text, action),
                        None => speak_clipboard(&shared, &config, Source::Page, action),
                    };
                    match result {
                        Ok(read) => {
                            reply.ok(read);
                            launcher::activate(&config);
                        }
                        Err(error) => {
                            let source = if text.is_some() {
                                Source::Text
                            } else {
                                Source::Page
                            };
                            reply.err(failed(&shared, source, &error));
                        }
                    }
                });
            }

            // L'adresse qui serait ouverte, sans rien lancer.
            "preview" => {
                let result = Self::config_param(&params).and_then(|config| {
                    let text = match params.get("text").and_then(Value::as_str) {
                        Some(text) => text.to_string(),
                        None => clipboard::read_text()?,
                    };
                    let prepared = config.prepare_text(&text);
                    let url = launcher::build_url(&config, &prepared);
                    Ok(json!({
                        "url": url,
                        "chars": prepared.chars().count(),
                        "length": url.len(),
                    }))
                });
                respond(reply, result);
            }

            "apps" => reply.ok(launcher::list_installed_apps()),

            "browse" => {
                std::thread::spawn(move || reply.ok(browse_for_browser()));
            }

            "hotkey.suspend" => {
                self.hotkeys().for_each(|hotkey| hotkey.suspend());
                reply.ok(Value::Null);
            }

            "hotkey.resume" => {
                self.hotkeys().for_each(|hotkey| hotkey.resume());
                reply.ok(Value::Null);
            }

            // Retire l'ancien programme, puis retente le raccourci qu'il
            // détenait sans doute.
            "legacy.remove" => {
                let hotkey = self.hotkey();
                std::thread::spawn(move || {
                    let removed = legacy::remove();
                    if let Some(hotkey) = &hotkey {
                        let spec = shared.config.lock().unwrap().hotkey.clone();
                        let _ = hotkey.set(&spec);
                    }
                    match removed {
                        Ok(()) => {
                            log_line!("ancien programme tts-hotkey retiré");
                            reply.ok(json!({
                                "legacy": legacy::traces(),
                                "hotkey": hotkey.map(|hotkey| hotkey.status()).unwrap_or_default(),
                            }));
                        }
                        Err(error) => reply.err(sentence(&format!("{error:#}"))),
                    }
                });
            }

            "open.data" => {
                let result = data_dir()
                    .and_then(|dir| {
                        std::fs::create_dir_all(&dir)
                            .with_context(|| format!("création de {}", dir.display()))?;
                        Ok(dir)
                    })
                    .and_then(|dir| crate::shell::open(&dir));
                respond(reply, result.map(|()| Value::Null));
            }

            other => reply.err(format!("méthode inconnue : {other}")),
        }
    }
}

impl Module for Speech {
    fn id(&self) -> &'static str {
        ID
    }

    fn script(&self) -> &'static str {
        include_str!("../../../ui/modules/speech.js")
    }

    fn default_widgets(&self) -> Vec<(&'static str, &'static str)> {
        vec![("speech.reader", "s")]
    }

    fn start(&self, context: ModuleContext) {
        *self.shared.host.lock().unwrap() = Some(context.host);

        let config = initial_config();
        let (read_spec, translate_spec) = (config.hotkey.clone(), config.translate_hotkey.clone());
        *self.shared.config.lock().unwrap() = config;

        *self.hotkey.lock().unwrap() = self.start_hotkey(&read_spec, Action::Read);
        *self.translate_hotkey.lock().unwrap() =
            self.start_hotkey(&translate_spec, Action::TranslateThenRead);
    }

    fn handle(&self, method: &str, params: Value, reply: Responder) {
        self.dispatch(method, params, reply);
    }

    fn notice_action(&self, _notice: &str, action: &str) {
        if action == "open"
            && let Some(host) = self.shared.host()
        {
            host.open(MAIN_PAGE);
        }
    }

    fn tray_status(&self) -> Option<String> {
        let status = self.hotkey_status();
        Some(match (status.active, status.suspended) {
            (Some(spec), false) => format!("Synthèse vocale\u{a0}: {spec}"),
            (Some(_), true) => "Synthèse vocale\u{a0}: raccourci suspendu".to_string(),
            (None, _) => "Synthèse vocale\u{a0}: aucun raccourci".to_string(),
        })
    }

    fn stop(&self) {
        if let Some(hotkey) = self.hotkey.lock().unwrap().take() {
            hotkey.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_app() -> InstalledApp {
        InstalledApp {
            id: "vivante".to_string(),
            name: "Just Tools".to_string(),
            profile: "Profile 1".to_string(),
            url: Some("https://just-tools.ascencia.re/".to_string()),
        }
    }

    #[test]
    fn a_dead_app_id_is_replaced() {
        let config = Config {
            app_id: Some("morte".into()),
            profile_directory: Some("Default".into()),
            ..Config::default()
        };
        let adopted = adopt_installed_app(config, Some(live_app()), false);
        assert_eq!(adopted.app_id.as_deref(), Some("vivante"));
        assert_eq!(adopted.profile_directory.as_deref(), Some("Profile 1"));
        assert_eq!(adopted.launch_mode, LaunchMode::Pwa);
    }

    #[test]
    fn a_choice_made_since_the_migration_is_kept() {
        let config = Config {
            schema: config::SCHEMA,
            launch_mode: LaunchMode::App,
            app_id: Some("vivante".into()),
            ..Config::default()
        };
        let adopted = adopt_installed_app(config, Some(live_app()), true);
        assert_eq!(adopted.launch_mode, LaunchMode::App);
    }

    #[test]
    fn nothing_changes_without_an_installed_app() {
        let adopted = adopt_installed_app(Config::default(), None, false);
        assert_eq!(adopted, Config::default());
    }

    #[test]
    fn bounding_counts_characters() {
        assert_eq!(bounded("été".to_string(), 2), ("ét".to_string(), true));
        assert_eq!(bounded("été".to_string(), 3), ("été".to_string(), false));
        assert_eq!(bounded("été".to_string(), 0), ("été".to_string(), false));
    }
}
