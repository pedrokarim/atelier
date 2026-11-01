//! Construction de l'adresse de lecture, ouverture dans Microsoft Edge et
//! déblocage de la lecture automatique.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Serialize;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SW_RESTORE,
    SetForegroundWindow, ShowWindow,
};
use windows::core::BOOL;

use super::config::{Config, LaunchMode};
use crate::logging::log_line;

const TOOL_PATH: &str = "/tools/text-to-speech";

/// Touche envoyée pour valider une interaction utilisateur auprès de Chromium.
/// F24 n'est présente sur aucun clavier courant : aucun risque d'effet de bord.
const VK_F24: u16 = 0x87;

/* --- Adresse ------------------------------------------------------------- */

/// Encode une valeur de paramètre d'URL (RFC 3986, caractères non réservés).
pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());

    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }

    out
}

/// Construit l'URL complète à ouvrir pour un texte donné.
///
/// Le texte voyage en base64url : bien plus compact que le percent-encoding
/// pour du français accentué (1,33x contre jusqu'à 9x par caractère
/// accentué), ce qui compte quand la ligne de commande Windows est limitée à
/// 32 767 caractères.
pub fn build_url(config: &Config, text: &str) -> String {
    let base = config.base_url.trim_end_matches('/');

    let mut url = String::with_capacity(text.len() * 2 + 64);
    url.push_str(base);
    if !base.ends_with(TOOL_PATH) {
        url.push_str(TOOL_PATH);
    }

    url.push_str("?b64=");
    url.push_str(&URL_SAFE_NO_PAD.encode(text.as_bytes()));

    if config.autoplay {
        url.push_str("&autoplay=1");
    }
    if config.reader_mode {
        url.push_str("&mini=1");
    }
    if let Some(rate) = config.rate {
        url.push_str(&format!("&rate={rate}"));
    }
    if let Some(pitch) = config.pitch {
        url.push_str(&format!("&pitch={pitch}"));
    }
    if let Some(volume) = config.volume {
        url.push_str(&format!("&volume={volume}"));
    }
    if let Some(voice) = config.voice.as_deref().filter(|v| !v.is_empty()) {
        url.push_str("&voice=");
        url.push_str(&percent_encode(voice));
    }
    if let Some(lang) = config.lang.as_deref().filter(|v| !v.is_empty()) {
        url.push_str("&lang=");
        url.push_str(&percent_encode(lang));
    }

    url
}

/* --- Edge ---------------------------------------------------------------- */

/// Une PWA Edge installée, telle que détectée sur le disque.
#[derive(Debug, Clone, Serialize)]
pub struct InstalledApp {
    pub id: String,
    pub name: String,
    pub profile: String,
    /// Adresse de départ, si le registre d'Edge la donne.
    pub url: Option<String>,
}

/// Emplacements habituels de msedge.exe.
fn edge_candidates() -> Vec<PathBuf> {
    ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(|variable| std::env::var(variable).ok())
        .map(|root| {
            PathBuf::from(root)
                .join("Microsoft")
                .join("Edge")
                .join("Application")
                .join("msedge.exe")
        })
        .collect()
}

/// Chemin de msedge.exe trouvé sans configuration.
pub fn detected_edge() -> Option<PathBuf> {
    edge_candidates().into_iter().find(|path| path.is_file())
}

/// Résout le chemin de msedge.exe (configuration puis détection).
pub fn resolve_edge(config: &Config) -> Result<PathBuf> {
    if let Some(configured) = config.edge_path.as_deref().filter(|p| !p.is_empty()) {
        let path = PathBuf::from(configured);
        if path.is_file() {
            return Ok(path);
        }
        return Err(anyhow!(
            "le navigateur indiqué n’existe pas\u{a0}: {configured}"
        ));
    }

    detected_edge().ok_or_else(|| {
        anyhow!("msedge.exe introuvable\u{a0}: indiquez le chemin du navigateur dans les réglages")
    })
}

fn edge_user_data() -> Option<PathBuf> {
    std::env::var("LOCALAPPDATA").ok().map(|root| {
        PathBuf::from(root)
            .join("Microsoft")
            .join("Edge")
            .join("User Data")
    })
}

/// Dossier qu'Edge tient à jour pour chaque PWA installée.
fn manifest_resources(profile: &Path) -> PathBuf {
    profile.join("Web Applications").join("Manifest Resources")
}

/// Une PWA désinstallée laisse son dossier `_crx__<id>` et son icône, mais
/// perd ses ressources de manifeste : c'est donc à elles qu'on reconnaît une
/// application qu'Edge sait encore lancer. Un chemin de navigateur
/// personnalisé peut désigner une autre édition d'Edge, aux données
/// ailleurs : on lui fait alors confiance.
pub fn is_installed(config: &Config) -> bool {
    let Some(id) = config.app_id.as_deref().filter(|id| !id.is_empty()) else {
        return false;
    };
    if config.edge_path.is_some() {
        return true;
    }
    let profile = config.profile_directory.as_deref().unwrap_or("Default");

    edge_user_data().is_some_and(|user_data| {
        manifest_resources(&user_data.join(profile))
            .join(id)
            .is_dir()
    })
}

/// Fichiers du registre des applications d'Edge, une base LevelDB. Edge la
/// garde ouverte, mais en autorise la lecture.
fn read_app_registry(profile: &Path) -> Vec<Vec<u8>> {
    let Ok(files) = std::fs::read_dir(profile.join("Sync Data").join("LevelDB")) else {
        return Vec::new();
    };

    files
        .flatten()
        .filter(|file| {
            let name = file.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".ldb") || name.ends_with(".log")
        })
        .filter_map(|file| std::fs::read(file.path()).ok())
        .collect()
}

/// Cherche l'adresse de départ d'une application dans un fichier du registre.
///
/// Chaque fiche place cette adresse juste après l'identifiant, et LevelDB
/// laisse lisible sa première apparition même compressée. Au-delà de
/// `URL_DISTANCE` octets, l'adresse est celle de la fiche suivante.
fn registered_url(registry: &[u8], id: &str) -> Option<String> {
    const URL_DISTANCE: usize = 16;

    let mut search = 0;
    while let Some(found) = find_bytes(&registry[search..], id.as_bytes()) {
        let start = search + found + id.len();
        let near = &registry[start..registry.len().min(start + URL_DISTANCE + 8)];

        if let Some(offset) = find_bytes(near, b"https://") {
            let url: String = registry[start + offset..]
                .iter()
                .take_while(|byte| byte.is_ascii_graphic())
                .map(|byte| *byte as char)
                .collect();
            return Some(url);
        }
        search = start;
    }

    None
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// L'hôte d'une adresse, en minuscules.
fn host_of(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let host = rest.split(['/', ':', '?', '#']).next()?;
    (!host.is_empty()).then(|| host.to_lowercase())
}

/// Le nom déposé par Edge à côté de l'icône de l'application, s'il y en a un.
fn icon_name(web_apps: &Path, id: &str) -> Option<String> {
    let files = std::fs::read_dir(web_apps.join(format!("_crx__{id}"))).ok()?;
    files.flatten().find_map(|file| {
        let file_name = file.file_name().to_string_lossy().to_string();
        file_name.strip_suffix(".ico").map(str::to_string)
    })
}

/// Liste les PWA installées, profil par profil.
pub fn list_installed_apps() -> Vec<InstalledApp> {
    let mut apps = Vec::new();

    let Some(profiles) = edge_user_data().and_then(|path| std::fs::read_dir(path).ok()) else {
        return apps;
    };

    for profile in profiles.flatten() {
        let profile_name = profile.file_name().to_string_lossy().to_string();
        if profile_name != "Default" && !profile_name.starts_with("Profile") {
            continue;
        }

        let path = profile.path();
        let Ok(entries) = std::fs::read_dir(manifest_resources(&path)) else {
            continue;
        };
        let registry = read_app_registry(&path);

        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().to_string();
            let url = registry.iter().find_map(|file| registered_url(file, &id));
            let name = icon_name(&path.join("Web Applications"), &id)
                .or_else(|| url.as_deref().and_then(host_of))
                .unwrap_or_else(|| "(nom inconnu)".to_string());

            apps.push(InstalledApp {
                id,
                name,
                profile: profile_name.clone(),
                url,
            });
        }
    }

    apps
}

/// Cherche, parmi les applications installées, celle du site `base_url`.
pub fn detect_app(base_url: &str) -> Option<InstalledApp> {
    let host = host_of(base_url)?;
    let apps = list_installed_apps();
    let by_host = apps
        .iter()
        .position(|app| app.url.as_deref().and_then(host_of).as_deref() == Some(host.as_str()));
    let by_name = || {
        apps.iter()
            .position(|app| app.name.to_lowercase().contains("just tools"))
    };

    by_host.or_else(by_name).map(|index| apps[index].clone())
}

/// Lance l'URL dans Edge selon le mode configuré.
pub fn launch(config: &Config, url: &str) -> Result<()> {
    let edge = resolve_edge(config)?;
    let mut command = Command::new(&edge);

    if let Some(profile) = config
        .profile_directory
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        command.arg(format!("--profile-directory={profile}"));
    }

    // Edge se termine sans rien dire sur un identifiant inconnu : mieux vaut
    // une fenêtre de plus qu'une lecture perdue.
    let mode = match config.launch_mode {
        LaunchMode::Pwa if !is_installed(config) => {
            log_line!("PWA introuvable : repli sur la fenêtre applicative");
            LaunchMode::App
        }
        other => other,
    };

    match mode {
        LaunchMode::Pwa => {
            let app_id = config.app_id.as_deref().unwrap_or_default();
            command.arg(format!("--app-id={app_id}"));
            command.arg(format!("--app-launch-url-for-shortcuts-menu-item={url}"));
        }
        LaunchMode::App => {
            command.arg(format!("--app={url}"));
        }
        LaunchMode::Tab => {
            command.arg(url);
        }
    }

    command
        .spawn()
        .with_context(|| format!("lancement de {}", edge.display()))?;

    Ok(())
}

/* --- Déblocage de la lecture automatique --------------------------------- */

struct Search {
    needle: String,
    own_pid: u32,
    found: Option<HWND>,
}

unsafe extern "system" fn find_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = unsafe { &mut *(lparam.0 as *mut Search) };

    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return BOOL(1);
    }

    // La fenêtre d'Atelier elle-même peut porter « Synthèse vocale » dans
    // son titre : elle n'est jamais la cible.
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == search.own_pid {
        return BOOL(1);
    }

    let mut buffer = [0u16; 512];
    let length = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    if length <= 0 {
        return BOOL(1);
    }

    let title = String::from_utf16_lossy(&buffer[..length as usize]).to_lowercase();
    if title.contains(&search.needle) {
        search.found = Some(hwnd);
        return BOOL(0);
    }

    BOOL(1)
}

/// Cherche une fenêtre visible, d'un autre programme, dont le titre contient
/// `needle`.
fn find_window(needle: &str) -> Option<HWND> {
    let mut search = Search {
        needle: needle.to_lowercase(),
        own_pid: std::process::id(),
        found: None,
    };

    // EnumWindows renvoie une erreur quand le rappel interrompt l'énumération :
    // ce n'est pas un échec ici.
    let _ = unsafe { EnumWindows(Some(find_proc), LPARAM(&mut search as *mut Search as isize)) };

    search.found
}

fn focus_window(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Envoie une frappe F24 : une vraie frappe clavier vaut geste utilisateur
/// pour Chromium, ce qui autorise `speechSynthesis.speak()` sur un document
/// neuf.
fn send_activation_key() {
    let keyboard_input = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(VK_F24),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let inputs = [
        keyboard_input(Default::default()),
        keyboard_input(KEYEVENTF_KEYUP),
    ];

    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        log_line!(
            "SendInput n'a envoyé que {sent} événement(s) sur {}",
            inputs.len()
        );
    }
}

/// Après le lancement : attendre l'ouverture, focaliser, débloquer
/// l'autoplay. À appeler depuis un fil qui peut attendre.
pub fn activate(config: &Config) {
    if !config.focus_window && !config.send_activation_key {
        return;
    }
    let needle = config.window_title_match.as_str();
    if needle.is_empty() {
        return;
    }

    thread::sleep(Duration::from_millis(config.activation_delay_ms));

    // La fenêtre peut mettre un peu plus longtemps à apparaître.
    let mut window = find_window(needle);
    for _ in 0..10 {
        if window.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(200));
        window = find_window(needle);
    }

    match window {
        Some(hwnd) => {
            if config.focus_window {
                focus_window(hwnd);
                thread::sleep(Duration::from_millis(120));
            }
            if config.send_activation_key {
                send_activation_key();
            }
            log_line!(
                "fenêtre « {needle} » trouvée (premier plan : {}, frappe d'activation : {})",
                config.focus_window,
                config.send_activation_key
            );
        }
        None => log_line!(
            "fenêtre « {needle} » introuvable : lecture automatique laissée au navigateur"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_tool_path_once() {
        let config = Config {
            base_url: "https://exemple.fr/tools/text-to-speech/".into(),
            reader_mode: false,
            ..Config::default()
        };
        let url = build_url(&config, "a");
        assert_eq!(url.matches("/tools/text-to-speech").count(), 1);
    }

    #[test]
    fn encodes_accents_in_base64url() {
        let config = Config {
            base_url: "https://exemple.fr".into(),
            autoplay: false,
            reader_mode: false,
            ..Config::default()
        };
        let url = build_url(&config, "Été");
        // Aucun caractère nécessitant un échappement d'URL.
        assert!(!url.contains('%'));
        assert!(!url.contains('+'));
        let encoded = url.split("b64=").nth(1).unwrap();
        let decoded = URL_SAFE_NO_PAD.decode(encoded).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), "Été");
    }

    const LIVE_ID: &str = "ljkcbphioghkcdigeialakdeoeehdlcp";

    #[test]
    fn reads_the_start_url_of_a_registry_record() {
        let mut registry = b"\x00dt-".to_vec();
        registry.extend_from_slice(LIVE_ID.as_bytes());
        registry.extend_from_slice(b":\xcf\x01\x123https://just-tools.ascencia.re/\x12.Just Tools");
        assert_eq!(
            registered_url(&registry, LIVE_ID).as_deref(),
            Some("https://just-tools.ascencia.re/")
        );
    }

    #[test]
    fn skips_records_without_a_nearby_url() {
        // Première mention sans adresse, seconde avec : la fiche utile est
        // la seconde.
        let mut registry = LIVE_ID.as_bytes().to_vec();
        registry.extend_from_slice(&[0; 64]);
        registry.extend_from_slice(LIVE_ID.as_bytes());
        registry.extend_from_slice(b"\x12https://exemple.fr/\x12");
        assert_eq!(
            registered_url(&registry, LIVE_ID).as_deref(),
            Some("https://exemple.fr/")
        );
    }

    #[test]
    fn ignores_the_url_of_the_next_record() {
        // Le cas d'une application désinstallée : l'adresse la plus proche
        // appartient à la fiche suivante.
        let mut registry = LIVE_ID.as_bytes().to_vec();
        registry.extend_from_slice(&[0; 54]);
        registry.extend_from_slice(b"https://autre.fr/");
        assert_eq!(registered_url(&registry, LIVE_ID), None);
    }

    #[test]
    fn extracts_hosts() {
        assert_eq!(
            host_of("https://Just-Tools.ascencia.re/t").as_deref(),
            Some("just-tools.ascencia.re")
        );
        assert_eq!(
            host_of("https://exemple.fr:8080").as_deref(),
            Some("exemple.fr")
        );
        assert_eq!(host_of("exemple.fr"), None);
    }

    #[test]
    fn an_app_id_without_edge_data_is_not_installed() {
        let config = Config {
            app_id: Some("aucune".into()),
            ..Config::default()
        };
        assert!(!is_installed(&config));
        assert!(
            is_installed(&Config {
                edge_path: Some(r"D:\edge\msedge.exe".into()),
                ..config
            }),
            "un navigateur personnalisé garde ses données ailleurs"
        );
        assert!(!is_installed(&Config::default()));
    }

    #[test]
    fn percent_encodes_voice_names() {
        let config = Config {
            base_url: "https://exemple.fr".into(),
            voice: Some("Microsoft Denise (Natural)".into()),
            ..Config::default()
        };
        let url = build_url(&config, "a");
        assert!(url.contains("&voice=Microsoft%20Denise%20%28Natural%29"));
        assert!(url.contains("&autoplay=1&mini=1"));
    }
}
