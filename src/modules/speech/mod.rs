//! Module « Synthèse vocale » : un raccourci global qui envoie le
//! presse-papiers vers la synthèse vocale de Just Tools.
//!
//! ```text
//! Ctrl+Alt+S → presse-papiers → base64url → Edge → /tools/text-to-speech?b64=…&autoplay=1
//! ```
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

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result, anyhow};
use serde::Serialize;
use serde_json::{Value, json};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OPENFILENAMEW};
use windows::Win32::UI::WindowsAndMessaging::MB_OK;
use windows::core::{PCWSTR, PWSTR};

use self::config::Config;
use self::hotkey::Hotkey;
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

/// La dernière lecture, montrée par la page et le widget.
#[derive(Debug, Clone, Serialize)]
struct LastRead {
    at: i64,
    source: Source,
    chars: usize,
    truncated: bool,
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

/// Envoie un texte vers l'outil. La lecture automatique reste à débloquer
/// ensuite, hors du fil de l'interface (`launcher::activate`).
fn speak(shared: &Shared, config: &Config, text: &str, source: Source) -> Result<LastRead> {
    let unbounded = Config {
        max_chars: 0,
        ..config.clone()
    }
    .prepare_text(text);
    if unbounded.is_empty() {
        return Err(anyhow!("rien à lire\u{a0}: le texte est vide"));
    }
    let truncated = config.max_chars > 0 && unbounded.chars().count() > config.max_chars;
    let prepared: String = if truncated {
        unbounded.chars().take(config.max_chars).collect()
    } else {
        unbounded
    };

    let target = launcher::build_url(config, &prepared);
    let chars = prepared.chars().count();
    log_line!(
        "synthèse vocale : {chars} caractères ({} octets d'adresse){}",
        target.len(),
        if truncated { ", texte tronqué" } else { "" }
    );

    launcher::launch(config, &target)?;

    let read = LastRead {
        at: now_ms(),
        source,
        chars,
        truncated,
        error: None,
    };
    shared.record(read.clone());
    Ok(read)
}

/// Lecture du presse-papiers, avec la configuration donnée.
fn speak_clipboard(shared: &Shared, config: &Config, source: Source) -> Result<LastRead> {
    if config.beep {
        unsafe {
            let _ = MessageBeep(MB_OK);
        }
    }

    let text = clipboard::read_text()?;
    if text.trim().is_empty() {
        return Err(anyhow!("le presse-papiers ne contient aucun texte"));
    }
    speak(shared, config, &text, source)
}

/// Appui sur le raccourci global : le fil du raccourci rend la main tout de
/// suite, la lecture se fait à côté.
fn on_hotkey(shared: &Arc<Shared>) {
    let shared = shared.clone();
    std::thread::spawn(move || {
        let config = shared.config.lock().unwrap().clone();
        match speak_clipboard(&shared, &config, Source::Hotkey) {
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

/// Réglages de démarrage : ceux du module, ou, au tout premier lancement,
/// ceux de l'ancien programme, ou les valeurs par défaut complétées de la PWA
/// Just Tools si Edge l'a installée.
fn initial_config() -> Config {
    if Config::exists() {
        return Config::load();
    }

    let config = legacy::import_config().unwrap_or_else(|| {
        let mut config = Config::default();
        if let Some(app) = launcher::detect_just_tools_app() {
            log_line!("PWA Just Tools détectée : {} ({})", app.id, app.profile);
            config.app_id = Some(app.id);
            config.profile_directory = Some(app.profile);
        }
        config
    });
    let config = config.normalized();
    if let Err(error) = config.save() {
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

pub struct Speech {
    shared: Arc<Shared>,
    hotkey: Mutex<Option<Arc<Hotkey>>>,
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
        }
    }

    fn hotkey(&self) -> Option<Arc<Hotkey>> {
        self.hotkey.lock().unwrap().clone()
    }

    fn hotkey_status(&self) -> hotkey::Status {
        self.hotkey()
            .map(|hotkey| hotkey.status())
            .unwrap_or_default()
    }

    fn state(&self) -> Value {
        json!({
            "config": *self.shared.config.lock().unwrap(),
            "hotkey": self.hotkey_status(),
            "last": *self.shared.last.lock().unwrap(),
            "legacy": legacy::traces(),
            "edge": launcher::detected_edge().map(|path| path.display().to_string()),
            "maxCharsLimit": config::MAX_CHARS_LIMIT,
        })
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
        if !config.hotkey.is_empty() {
            hotkey::parse(&config.hotkey)
                .map_err(|error| anyhow!("raccourci invalide\u{a0}: {error:#}"))?;
        }
        Ok(config)
    }

    /// Enregistre les réglages. Un raccourci refusé par Windows n'empêche
    /// pas d'enregistrer le reste : l'ancien est conservé, et la réponse le
    /// dit.
    fn save(&self, params: &Value) -> Result<Value> {
        let mut config = Self::config_param(params)?;
        let mut warning = None;

        if let Some(hotkey) = self.hotkey()
            && let Err(error) = hotkey.set(&config.hotkey)
        {
            if let Some(active) = hotkey.status().active {
                config.hotkey = active;
            }
            warning = Some(sentence(&error));
        }

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

                std::thread::spawn(move || {
                    let result = match &text {
                        Some(text) => speak(&shared, &config, text, Source::Text),
                        None => speak_clipboard(&shared, &config, Source::Page),
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
                if let Some(hotkey) = self.hotkey() {
                    hotkey.suspend();
                }
                reply.ok(Value::Null);
            }

            "hotkey.resume" => {
                if let Some(hotkey) = self.hotkey() {
                    hotkey.resume();
                }
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
        let spec = config.hotkey.clone();
        *self.shared.config.lock().unwrap() = config;

        let shared = self.shared.clone();
        let hotkey = match Hotkey::start(move || on_hotkey(&shared)) {
            Ok(hotkey) => Arc::new(hotkey),
            Err(error) => {
                log_line!("raccourci de synthèse vocale indisponible : {error:#}");
                return;
            }
        };

        if let Err(error) = hotkey.set(&spec) {
            let traces = legacy::traces();
            let body = if traces.running > 0 {
                format!(
                    "{} L’ancien programme tts-hotkey tourne encore et le détient\u{a0}: retirez-le depuis la page Synthèse vocale.",
                    sentence(&error)
                )
            } else {
                sentence(&error)
            };
            self.shared.notify(
                "hotkey",
                "Raccourci de synthèse vocale indisponible",
                body,
                "warning",
                None,
            );
        }

        *self.hotkey.lock().unwrap() = Some(hotkey);
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
