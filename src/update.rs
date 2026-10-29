//! Mises à jour : la dernière version publiée sur GitHub, et son installation.
//!
//! ```text
//! GitHub releases/latest → version plus récente ? → notification
//!   « Mettre à jour » → atelier-<v>-setup.exe téléchargé, vérifié par SHA256SUMS.txt
//!   → installateur silencieux lancé → Atelier se ferme → l'installateur le relance
//! ```
//!
//! Rien n'est envoyé : la vérification est une simple lecture de la page des
//! versions. Elle se coupe dans les réglages.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};

use crate::ipc::{self, HostSender, Notice, NoticeAction, Responder};
use crate::logging::log_line;
use crate::{http, install, paths};

const REPOSITORY: &str = "pedrokarim/atelier";
/// Version en cours. En développement, `ATELIER_PRETEND_VERSION` la remplace,
/// pour essayer la mise à jour sans publier de version plus ancienne.
fn current() -> String {
    #[cfg(debug_assertions)]
    if let Ok(version) = std::env::var("ATELIER_PRETEND_VERSION") {
        return version;
    }
    env!("CARGO_PKG_VERSION").to_string()
}

/// Première vérification peu après le démarrage, pour ne pas le ralentir.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(60);
const CHECK_EVERY_MS: i64 = 12 * 3600 * 1000;
const WAKE_EVERY: Duration = Duration::from_secs(15 * 60);

/// Identifiant de la notification, dans l'espace de l'hôte.
const NOTICE_ID: &str = "core:update";

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// « 0.3.1 » → (0, 3, 1). Une étiquette mal formée ne passe jamais pour plus
/// récente.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.trim().trim_start_matches('v');
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

/// Une version publiée, telle que l'interface la montre.
#[derive(Debug, Clone, Serialize)]
pub struct Release {
    pub version: String,
    /// Page de la version sur GitHub.
    pub url: String,
    pub published: Option<String>,
    #[serde(skip)]
    setup: Option<String>,
    #[serde(skip)]
    setup_name: Option<String>,
    #[serde(skip)]
    sums: Option<String>,
}

/// Lit la réponse de `releases/latest`.
fn parse_release(value: &Value) -> Result<Release> {
    let tag = value["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow!("réponse de GitHub illisible"))?;
    let version = tag.trim_start_matches('v').to_string();
    let setup_name = format!("atelier-{version}-setup.exe");

    let asset = |name: &str| {
        value["assets"].as_array().and_then(|assets| {
            assets
                .iter()
                .find(|asset| asset["name"].as_str() == Some(name))
                .and_then(|asset| asset["browser_download_url"].as_str())
                .map(str::to_string)
        })
    };

    Ok(Release {
        url: value["html_url"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("https://github.com/{REPOSITORY}/releases/tag/{tag}")),
        published: value["published_at"].as_str().map(str::to_string),
        setup: asset(&setup_name),
        sums: asset("SHA256SUMS.txt"),
        setup_name: Some(setup_name),
        version,
    })
}

/// Empreinte attendue d'un fichier, d'après un `SHA256SUMS.txt`.
fn expected_hash(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file == name && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

fn sha256_hex(data: &[u8]) -> Result<String> {
    let mut digest = [0u8; 32];
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut digest) };
    status.ok().context("calcul de l’empreinte")?;
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Ce qui est conservé d'une session à l'autre, dans `update.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    auto_check: bool,
    last_check_ms: Option<i64>,
    /// Dernière version annoncée par une notification : on ne la répète pas.
    notified: Option<String>,
    /// Version en cours d'installation, pour saluer son premier lancement.
    installing: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_check: true,
            last_check_ms: None,
            notified: None,
            installing: None,
        }
    }
}

impl Settings {
    fn path() -> Result<PathBuf> {
        Ok(paths::data_dir()?.join("update.json"))
    }

    fn load() -> Settings {
        Self::path()
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        let result = Self::path().and_then(|path| {
            let text = serde_json::to_vec_pretty(self)?;
            paths::write_atomic(&path, &text)
        });
        if let Err(error) = result {
            log_line!("réglages des mises à jour non enregistrés : {error:#}");
        }
    }
}

#[derive(Default)]
struct State {
    settings: Settings,
    latest: Option<Release>,
    checking: bool,
    installing: bool,
    error: Option<String>,
}

pub struct Updater {
    state: Mutex<State>,
    host: HostSender,
}

/// Message d'erreur en phrase : majuscule initiale, point final.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

/// Seule une copie installée se met à jour par l'installateur : une copie
/// portable ou de développement serait remplacée par une autre, ailleurs.
fn installable() -> Result<(), &'static str> {
    if paths::is_portable() {
        return Err("Copie portable\u{a0}: téléchargez la nouvelle archive.");
    }
    match install::status() {
        Ok(status) if status.running_from_install => Ok(()),
        _ => Err("Cette copie n’est pas la copie installée."),
    }
}

impl Updater {
    pub fn new(host: HostSender) -> Arc<Updater> {
        Arc::new(Updater {
            state: Mutex::new(State {
                settings: Settings::load(),
                ..State::default()
            }),
            host,
        })
    }

    /// Salue une mise à jour réussie, puis vérifie régulièrement.
    pub fn start(self: &Arc<Self>) {
        {
            let mut state = self.state.lock().unwrap();
            if let Some(version) = state.settings.installing.take() {
                let current = current();
                if version == current {
                    log_line!("mise à jour terminée : version {current}");
                    self.host.notify(Notice {
                        id: NOTICE_ID.to_string(),
                        module: "core".to_string(),
                        title: format!("Atelier est passé en version {current}"),
                        body: "La mise à jour s’est bien déroulée.".to_string(),
                        icon: None,
                        glyph: Some("check"),
                        tone: "info",
                        countdown: None,
                        timeout: Some(8),
                        actions: vec![NoticeAction::new("release", "Voir les nouveautés", false)],
                    });
                    state.latest = None;
                } else {
                    log_line!("mise à jour vers {version} non aboutie : version {current}");
                }
                state.settings.save();
            }
        }

        let updater = self.clone();
        let spawned = std::thread::Builder::new()
            .name("update".to_string())
            .spawn(move || {
                std::thread::sleep(FIRST_CHECK_AFTER);
                loop {
                    let due = {
                        let state = updater.state.lock().unwrap();
                        state.settings.auto_check
                            && state
                                .settings
                                .last_check_ms
                                .is_none_or(|last| now_ms() - last >= CHECK_EVERY_MS)
                    };
                    if due && let Err(error) = updater.check(true) {
                        log_line!("vérification des mises à jour : {error:#}");
                    }
                    std::thread::sleep(WAKE_EVERY);
                }
            });
        if let Err(error) = spawned {
            log_line!("vérification des mises à jour non démarrée : {error}");
        }
    }

    pub fn status(&self) -> Value {
        let state = self.state.lock().unwrap();
        let current = current();
        let available = state
            .latest
            .as_ref()
            .filter(|release| is_newer(&release.version, &current));
        json!({
            "current": current,
            "latest": state.latest,
            "available": available,
            "blocked": installable().err(),
            "checking": state.checking,
            "installing": state.installing,
            "error": state.error,
            "autoCheck": state.settings.auto_check,
            "lastCheck": state.settings.last_check_ms,
        })
    }

    fn changed(&self) {
        self.host.broadcast("core.update", self.status());
    }

    /// Interroge GitHub. Avec `notify`, une version nouvelle est annoncée
    /// par une notification, une seule fois par version.
    pub fn check(&self, notify: bool) -> Result<()> {
        {
            let mut state = self.state.lock().unwrap();
            if state.checking {
                return Ok(());
            }
            state.checking = true;
        }
        self.changed();

        let result = http::get(
            &format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"),
            "application/vnd.github+json",
            4 * 1024 * 1024,
        )
        .and_then(|body| {
            let value: Value =
                serde_json::from_slice(&body).context("réponse de GitHub illisible")?;
            parse_release(&value)
        });

        let mut announce = None;
        {
            let mut state = self.state.lock().unwrap();
            state.checking = false;
            match &result {
                Ok(release) => {
                    state.error = None;
                    state.settings.last_check_ms = Some(now_ms());
                    if is_newer(&release.version, &current()) {
                        if notify && state.settings.notified.as_deref() != Some(&release.version) {
                            state.settings.notified = Some(release.version.clone());
                            announce = Some(release.clone());
                        }
                        log_line!("mise à jour disponible : {}", release.version);
                    }
                    state.latest = Some(release.clone());
                    state.settings.save();
                }
                Err(error) => state.error = Some(sentence(&format!("{error:#}"))),
            }
        }
        self.changed();

        if let Some(release) = announce {
            let ready = installable().is_ok();
            let current = current();
            self.host.notify(Notice {
                id: NOTICE_ID.to_string(),
                module: "core".to_string(),
                title: format!("Atelier {} est disponible", release.version),
                body: if ready {
                    format!(
                        "Vous utilisez la version {current}. La mise à jour ferme Atelier un instant, puis le relance."
                    )
                } else {
                    format!("Vous utilisez la version {current}.")
                },
                icon: None,
                glyph: Some("download"),
                tone: "info",
                countdown: None,
                timeout: None,
                actions: if ready {
                    vec![
                        NoticeAction::new("dismiss", "Plus tard", false),
                        NoticeAction::new("install", "Mettre à jour", true),
                    ]
                } else {
                    vec![
                        NoticeAction::new("dismiss", "Plus tard", false),
                        NoticeAction::new("release", "Voir la version", true),
                    ]
                },
            });
        }

        result.map(|_| ())
    }

    /// Télécharge l'installateur, vérifie son empreinte et le lance. Atelier
    /// se ferme ensuite : l'installateur le relance une fois la copie faite.
    pub fn install(&self) -> Result<()> {
        installable().map_err(|reason| anyhow!("{reason}"))?;

        let release = {
            let mut state = self.state.lock().unwrap();
            if state.installing {
                return Err(anyhow!("mise à jour déjà en cours"));
            }
            let release = state
                .latest
                .clone()
                .filter(|release| is_newer(&release.version, &current()))
                .ok_or_else(|| anyhow!("aucune version plus récente connue"))?;
            state.installing = true;
            release
        };
        self.changed();

        let result = self.download_and_launch(&release);
        if let Err(error) = &result {
            let mut state = self.state.lock().unwrap();
            state.installing = false;
            state.error = Some(sentence(&format!("{error:#}")));
            drop(state);
            self.changed();
        }
        result
    }

    fn download_and_launch(&self, release: &Release) -> Result<()> {
        let (Some(setup_url), Some(name)) = (&release.setup, &release.setup_name) else {
            return Err(anyhow!("cette version ne propose pas d’installateur"));
        };
        let sums_url = release
            .sums
            .as_ref()
            .ok_or_else(|| anyhow!("cette version ne publie pas ses empreintes"))?;

        log_line!("téléchargement de la version {}", release.version);
        let sums = http::get(sums_url, "*/*", 64 * 1024)?;
        let expected = expected_hash(&String::from_utf8_lossy(&sums), name)
            .ok_or_else(|| anyhow!("empreinte de {name} introuvable"))?;
        let setup = http::get(setup_url, "application/octet-stream", 200 * 1024 * 1024)?;
        let actual = sha256_hex(&setup)?;
        if actual != expected {
            return Err(anyhow!(
                "l’installateur téléchargé ne correspond pas à son empreinte : mise à jour annulée"
            ));
        }

        let folder = std::env::temp_dir().join("atelier-update");
        std::fs::create_dir_all(&folder)
            .with_context(|| format!("création de {}", folder.display()))?;
        let path = folder.join(name);
        std::fs::write(&path, &setup).with_context(|| format!("écriture de {}", path.display()))?;

        {
            let mut state = self.state.lock().unwrap();
            state.settings.installing = Some(release.version.clone());
            state.settings.save();
        }

        std::process::Command::new(&path)
            .args([
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/RELAUNCH=1",
                &format!("/LOG={}", folder.join("setup.log").display()),
            ])
            .spawn()
            .with_context(|| format!("lancement de {}", path.display()))?;

        log_line!(
            "installateur {} lancé : fermeture pour mise à jour",
            release.version
        );
        ipc::post_to_host(ipc::WM_APP_QUIT, 0);
        Ok(())
    }

    fn open_release(&self) {
        let url = self
            .state
            .lock()
            .unwrap()
            .latest
            .as_ref()
            .map(|release| release.url.clone())
            .unwrap_or_else(|| format!("https://github.com/{REPOSITORY}/releases/latest"));
        if let Err(error) = crate::shell::open(std::path::Path::new(&url)) {
            log_line!("{error:#}");
        }
    }

    fn install_in_background(self: &Arc<Self>) {
        let updater = self.clone();
        std::thread::spawn(move || {
            if let Err(error) = updater.install() {
                log_line!("mise à jour impossible : {error:#}");
                updater.host.notify(Notice {
                    id: NOTICE_ID.to_string(),
                    module: "core".to_string(),
                    title: "Mise à jour impossible".to_string(),
                    body: sentence(&format!("{error:#}")),
                    icon: None,
                    glyph: Some("alert"),
                    tone: "warning",
                    countdown: None,
                    timeout: None,
                    actions: vec![NoticeAction::new("release", "Voir la version", true)],
                });
            }
        });
    }

    /// Bouton d'une notification de l'hôte.
    pub fn notice_action(self: &Arc<Self>, notice: &str, action: &str) {
        if notice != NOTICE_ID {
            return;
        }
        match action {
            "install" => self.install_in_background(),
            "release" => self.open_release(),
            _ => {}
        }
    }

    /// Requêtes `update.*` de la page des réglages. Le réseau se fait hors
    /// du fil de l'interface.
    pub fn handle(self: &Arc<Self>, method: &str, params: Value, reply: Responder) {
        match method {
            "update.status" => reply.ok(self.status()),
            "update.check" => {
                let updater = self.clone();
                std::thread::spawn(move || {
                    let result = updater.check(false);
                    match result {
                        Ok(()) => reply.ok(updater.status()),
                        Err(error) => reply.err(sentence(&format!("{error:#}"))),
                    }
                });
            }
            "update.install" => {
                let updater = self.clone();
                std::thread::spawn(move || match updater.install() {
                    Ok(()) => reply.ok(Value::Null),
                    Err(error) => reply.err(sentence(&format!("{error:#}"))),
                });
            }
            "update.release" => {
                self.open_release();
                reply.ok(Value::Null);
            }
            "update.auto" => {
                let enabled = params
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                {
                    let mut state = self.state.lock().unwrap();
                    state.settings.auto_check = enabled;
                    state.settings.save();
                }
                reply.ok(self.status());
            }
            other => reply.err(format!("méthode inconnue : {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.3.1", "0.3.0"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
        assert!(!is_newer("nimporte", "0.3.0"));
        assert_eq!(parse_version("1.2.3-rc.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
    }

    #[test]
    fn reads_a_github_release() {
        let value = json!({
            "tag_name": "v0.4.0",
            "html_url": "https://github.com/pedrokarim/atelier/releases/tag/v0.4.0",
            "published_at": "2026-11-02T10:00:00Z",
            "assets": [
                { "name": "atelier-0.4.0-portable.zip", "browser_download_url": "https://exemple/zip" },
                { "name": "atelier-0.4.0-setup.exe", "browser_download_url": "https://exemple/setup" },
                { "name": "SHA256SUMS.txt", "browser_download_url": "https://exemple/sums" }
            ]
        });
        let release = parse_release(&value).unwrap();
        assert_eq!(release.version, "0.4.0");
        assert_eq!(release.setup.as_deref(), Some("https://exemple/setup"));
        assert_eq!(release.sums.as_deref(), Some("https://exemple/sums"));
    }

    #[test]
    fn finds_the_expected_hash() {
        let sums = "AB12cd34ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34  atelier-0.4.0-setup.exe\n\
                    0000000000000000000000000000000000000000000000000000000000000000  atelier-0.4.0-portable.zip\n";
        assert_eq!(
            expected_hash(sums, "atelier-0.4.0-setup.exe").as_deref(),
            Some("ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34")
        );
        assert_eq!(expected_hash(sums, "autre.exe"), None);
    }

    #[test]
    fn hashes_with_sha256() {
        assert_eq!(
            sha256_hex(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// Réseau : la dernière version publiée se lit, et son installateur
    /// correspond à son empreinte. `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn downloads_and_verifies_the_latest_release() {
        let body = http::get(
            &format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"),
            "application/vnd.github+json",
            4 * 1024 * 1024,
        )
        .unwrap();
        let release = parse_release(&serde_json::from_slice(&body).unwrap()).unwrap();
        let sums = http::get(release.sums.as_ref().unwrap(), "*/*", 64 * 1024).unwrap();
        let expected = expected_hash(
            &String::from_utf8_lossy(&sums),
            release.setup_name.as_ref().unwrap(),
        )
        .unwrap();
        let setup = http::get(release.setup.as_ref().unwrap(), "*/*", 200 * 1024 * 1024).unwrap();
        assert_eq!(sha256_hex(&setup).unwrap(), expected);
    }
}
