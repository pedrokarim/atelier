//! L'ancien programme `tts-hotkey`, que ce module remplace.
//!
//! ```text
//! %APPDATA%\just-tools\tts-hotkey.toml                     réglages, repris au premier lancement
//! %LOCALAPPDATA%\Programs\tts-hotkey\                      copie installée
//! …\Menu Démarrer\Programmes\Synthèse vocale (Just Tools).lnk
//! HKCU\…\Run\just-tools-tts-hotkey                         démarrage automatique
//! ```
//!
//! Tant qu'il tourne, il détient le raccourci : Windows refuse de le donner
//! à un second programme. Le module le repère et propose de le retirer.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use serde_json::{Map, Value};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

use super::config::Config;
use crate::logging::log_line;

const EXECUTABLE_NAME: &str = "tts-hotkey.exe";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "just-tools-tts-hotkey";
const SHORTCUT_NAME: &str = "Synthèse vocale (Just Tools).lnk";

fn config_file() -> Option<PathBuf> {
    dirs::data_dir().map(|base| base.join("just-tools").join("tts-hotkey.toml"))
}

fn install_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|base| base.join("Programs").join("tts-hotkey"))
}

fn shortcut() -> Option<PathBuf> {
    dirs::data_dir().map(|base| {
        base.join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join(SHORTCUT_NAME)
    })
}

/* --- Reprise des réglages ------------------------------------------------ */

/// Valeur TOML d'une ligne `clé = valeur`. Le fichier de l'ancien programme
/// est plat et écrit par lui : chaînes, booléens et nombres suffisent.
fn parse_value(raw: &str) -> Option<Value> {
    let raw = raw.trim();

    if let Some(body) = raw.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = body.chars();
        while let Some(character) = chars.next() {
            match character {
                '"' => return Some(Value::String(out)),
                '\\' => out.push(match chars.next()? {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                }),
                other => out.push(other),
            }
        }
        return None;
    }

    // Un commentaire peut suivre la valeur.
    let raw = raw.split('#').next()?.trim();
    match raw {
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        _ => {
            if let Ok(integer) = raw.parse::<u64>() {
                Some(Value::from(integer))
            } else {
                raw.parse::<f64>().ok().map(Value::from)
            }
        }
    }
}

fn parse_config(text: &str) -> Result<Config> {
    let mut fields = Map::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if let Some(value) = parse_value(value) {
            fields.insert(key.trim().to_string(), value);
        }
    }

    serde_json::from_value(Value::Object(fields)).context("réglages de tts-hotkey illisibles")
}

/// Réglages de l'ancien programme, s'il en a laissé.
pub fn import_config() -> Option<Config> {
    let path = config_file()?;
    let text = std::fs::read_to_string(&path).ok()?;
    match parse_config(&text) {
        Ok(config) => {
            log_line!("réglages repris de {}", path.display());
            Some(config)
        }
        Err(error) => {
            log_line!("{error:#}");
            None
        }
    }
}

/* --- Repérage et retrait ------------------------------------------------- */

/// Ce qui reste de l'ancien programme sur l'ordinateur.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Traces {
    /// Processus `tts-hotkey.exe` en cours.
    pub running: usize,
    pub installed: Option<String>,
    pub autostart: bool,
    pub shortcut: bool,
}

fn processes() -> Vec<u32> {
    let mut found = Vec::new();

    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return found;
        };

        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let length = entry
                    .szExeFile
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
                if name.eq_ignore_ascii_case(EXECUTABLE_NAME) {
                    found.push(entry.th32ProcessID);
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
    }

    found
}

fn run_key(access: u32) -> std::io::Result<RegKey> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, access)
}

pub fn traces() -> Traces {
    Traces {
        running: processes().len(),
        installed: install_dir()
            .filter(|dir| dir.exists())
            .map(|dir| dir.display().to_string()),
        autostart: run_key(KEY_READ)
            .and_then(|key| key.get_value::<String, _>(RUN_VALUE))
            .is_ok(),
        shortcut: shortcut().is_some_and(|path| path.exists()),
    }
}

/// Arrête l'ancien programme et retire son installation, son raccourci du
/// menu Démarrer et son démarrage automatique. Ses réglages et son journal,
/// dans `%APPDATA%\just-tools`, restent en place.
pub fn remove() -> Result<()> {
    let mut errors: Vec<String> = Vec::new();

    for pid in processes() {
        let stopped = unsafe {
            OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid).map(|handle| {
                let result = TerminateProcess(handle, 0);
                // Les fichiers de l'exécutable ne se suppriment qu'une fois
                // le processus vraiment terminé.
                WaitForSingleObject(handle, 3_000);
                let _ = CloseHandle(handle);
                result
            })
        };
        match stopped {
            Ok(Ok(())) => log_line!("ancien tts-hotkey arrêté ({pid})"),
            _ => errors.push(format!("le processus {pid} n’a pas pu être arrêté")),
        }
    }

    if let Ok(key) = run_key(KEY_WRITE) {
        match key.delete_value(RUN_VALUE) {
            Ok(()) => log_line!("démarrage automatique de tts-hotkey retiré"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => errors.push(format!("démarrage automatique non retiré ({error})")),
        }
    }

    if let Some(path) = shortcut().filter(|path| path.exists())
        && let Err(error) = std::fs::remove_file(&path)
    {
        errors.push(format!("raccourci du menu Démarrer non retiré ({error})"));
    }

    if let Some(dir) = install_dir().filter(|dir| dir.exists()) {
        // Le système relâche parfois l'exécutable un instant après l'arrêt.
        let mut result = std::fs::remove_dir_all(&dir);
        for _ in 0..5 {
            if result.is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(300));
            result = std::fs::remove_dir_all(&dir);
        }
        match result {
            Ok(()) => log_line!("installation de tts-hotkey retirée : {}", dir.display()),
            Err(error) => errors.push(format!("{} non supprimé ({error})", dir.display())),
        }
    }

    match errors.first() {
        None => Ok(()),
        Some(_) => Err(anyhow!("{}", errors.join("\u{a0}; "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::speech::config::LaunchMode;

    #[test]
    fn imports_the_old_configuration_file() {
        let text = r#"
# Configuration de tts-hotkey
base_url = "https://just-tools.ascencia.re"
hotkey = "Ctrl+Shift+F9"
launch_mode = "pwa"
app_id = "hbblfifohofgngfbjbiimbbcimepbdcb"
# edge_path = "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe"
edge_path = "C:\\Edge\\msedge.exe"
rate = 1.2
volume = 1.0
voice = "Denise \"Natural\""
max_chars = 4242
dehyphenate = true
window_title_match = "Synthèse Vocale"
beep = false
"#;
        let config = parse_config(text).unwrap();
        assert_eq!(config.hotkey, "Ctrl+Shift+F9");
        assert_eq!(config.launch_mode, LaunchMode::Pwa);
        assert_eq!(
            config.app_id.as_deref(),
            Some("hbblfifohofgngfbjbiimbbcimepbdcb")
        );
        assert_eq!(config.edge_path.as_deref(), Some(r"C:\Edge\msedge.exe"));
        assert_eq!(config.rate, Some(1.2));
        assert_eq!(config.volume, Some(1.0));
        assert_eq!(config.voice.as_deref(), Some(r#"Denise "Natural""#));
        assert_eq!(config.max_chars, 4242);
        assert!(config.dehyphenate);
        assert!(!config.beep);
        assert!(config.autoplay);
    }
}
