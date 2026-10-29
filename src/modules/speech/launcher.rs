//! Construction de l'adresse de lecture, ouverture dans Microsoft Edge et
//! déblocage de la lecture automatique.

use std::path::PathBuf;
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
fn percent_encode(value: &str) -> String {
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

/// Liste les PWA installées en inspectant les dossiers `Web Applications`
/// d'Edge. Le nom de l'application est déduit du fichier `.ico` déposé par
/// Edge.
pub fn list_installed_apps() -> Vec<InstalledApp> {
    let mut apps = Vec::new();

    let Some(user_data) = std::env::var("LOCALAPPDATA").ok().map(|root| {
        PathBuf::from(root)
            .join("Microsoft")
            .join("Edge")
            .join("User Data")
    }) else {
        return apps;
    };

    let Ok(profiles) = std::fs::read_dir(&user_data) else {
        return apps;
    };

    for profile in profiles.flatten() {
        let profile_name = profile.file_name().to_string_lossy().to_string();
        if profile_name != "Default" && !profile_name.starts_with("Profile") {
            continue;
        }

        let Ok(entries) = std::fs::read_dir(profile.path().join("Web Applications")) else {
            continue;
        };

        for entry in entries.flatten() {
            let folder = entry.file_name().to_string_lossy().to_string();
            // Edge nomme les dossiers `_crx__<app_id>`.
            let Some(id) = folder
                .strip_prefix("_crx__")
                .or_else(|| folder.strip_prefix("_crx_"))
            else {
                continue;
            };

            let name = std::fs::read_dir(entry.path())
                .ok()
                .and_then(|files| {
                    files.flatten().find_map(|file| {
                        let file_name = file.file_name().to_string_lossy().to_string();
                        file_name.strip_suffix(".ico").map(str::to_string)
                    })
                })
                .unwrap_or_else(|| "(nom inconnu)".to_string());

            apps.push(InstalledApp {
                id: id.to_string(),
                name,
                profile: profile_name.clone(),
            });
        }
    }

    apps
}

/// Cherche la PWA Just Tools parmi les applications installées.
pub fn detect_just_tools_app() -> Option<InstalledApp> {
    list_installed_apps()
        .into_iter()
        .find(|app| app.name.to_lowercase().contains("just tools"))
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

    let mode = match config.launch_mode {
        LaunchMode::Pwa if config.app_id.as_deref().unwrap_or("").is_empty() => {
            log_line!("mode PWA sans identifiant d'application : repli sur la fenêtre applicative");
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
