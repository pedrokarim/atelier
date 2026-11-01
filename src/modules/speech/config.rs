//! Réglages du module, enregistrés dans `config.json`.
//!
//! Les noms des champs sont ceux de l'ancien `tts-hotkey.toml` : le fichier
//! de l'ancien programme se reprend tel quel (voir `legacy`).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths;

/// Comment ouvrir l'outil dans Microsoft Edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchMode {
    /// PWA installée, identifiée par son `app_id` (fenêtre réutilisée).
    Pwa,
    /// Fenêtre applicative sans barre d'adresse (`--app=`), sans installation.
    App,
    /// Onglet classique du navigateur.
    Tab,
}

/// Version du format des réglages. Un fichier plus ancien (0) n'a jamais eu
/// l'occasion de choisir la PWA installée : voir `adopt_installed_app`.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema: u32,
    /// Racine du site, ou URL complète de l'outil.
    pub base_url: String,
    /// Raccourci global, par exemple « Ctrl+Alt+S » ; vide, aucun raccourci.
    pub hotkey: String,
    /// Raccourci qui traduit avant de lire ; vide, aucun raccourci.
    pub translate_hotkey: String,
    /// Langue vers laquelle traduire (« fr », « en »…).
    pub translate_target: String,
    pub launch_mode: LaunchMode,
    /// Chemin de msedge.exe ; détecté automatiquement si absent.
    pub edge_path: Option<String>,
    /// Identifiant de la PWA installée (voir edge://apps), requis en mode `pwa`.
    pub app_id: Option<String>,
    /// Profil Edge à utiliser, par exemple « Default » ou « Profile 1 ».
    pub profile_directory: Option<String>,

    /// Démarrer la lecture dès l'ouverture.
    pub autoplay: bool,
    /// Ouvrir l'interface « lecteur » épurée.
    pub reader_mode: bool,
    pub rate: Option<f32>,
    pub pitch: Option<f32>,
    pub volume: Option<f32>,
    /// Nom exact ou partiel d'une voix (« Denise », « Natural »…).
    pub voice: Option<String>,
    /// Langue forcée (« fr-FR », « en-US »…).
    pub lang: Option<String>,

    /// Longueur maximale envoyée ; au-delà le texte est tronqué.
    pub max_chars: usize,
    /// Aplatir les retours à la ligne et espaces multiples (utile après un OCR).
    pub collapse_whitespace: bool,
    /// Recoller les mots coupés en fin de ligne (« exem-\nple » → « exemple »).
    pub dehyphenate: bool,

    /// Mettre la fenêtre au premier plan après le lancement.
    pub focus_window: bool,
    /// Envoyer une frappe inoffensive pour débloquer la politique d'autoplay
    /// de Chromium (une vraie frappe clavier compte comme geste utilisateur).
    pub send_activation_key: bool,
    /// Délai avant la mise au premier plan et la frappe, en millisecondes.
    pub activation_delay_ms: u64,
    /// Fragment de titre identifiant la fenêtre de l'outil.
    pub window_title_match: String,
    /// Bip système à chaque déclenchement.
    pub beep: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema: 0,
            base_url: "https://just-tools.ascencia.re".to_string(),
            hotkey: "Ctrl+Alt+S".to_string(),
            translate_hotkey: "Ctrl+Alt+T".to_string(),
            translate_target: DEFAULT_TRANSLATE_TARGET.to_string(),
            launch_mode: LaunchMode::App,
            edge_path: None,
            app_id: None,
            profile_directory: None,
            autoplay: true,
            reader_mode: true,
            rate: None,
            pitch: None,
            volume: None,
            voice: None,
            lang: None,
            max_chars: 12_000,
            collapse_whitespace: true,
            dehyphenate: false,
            focus_window: true,
            send_activation_key: true,
            activation_delay_ms: 1_200,
            window_title_match: "Synthèse Vocale".to_string(),
            beep: true,
        }
    }
}

/// Plafond de `max_chars` : la ligne de commande Windows est limitée à
/// 32 767 caractères, et le base64 gonfle un texte accentué.
pub const MAX_CHARS_LIMIT: usize = 16_000;

const DEFAULT_TRANSLATE_TARGET: &str = "fr";

impl Config {
    fn path() -> Result<std::path::PathBuf> {
        Ok(super::data_dir()?.join("config.json"))
    }

    pub fn exists() -> bool {
        Self::path().is_ok_and(|path| path.exists())
    }

    /// Un fichier illisible est mis de côté : les valeurs par défaut
    /// s'appliquent.
    pub fn load() -> Config {
        let Ok(path) = Self::path() else {
            return Config::default();
        };

        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
                crate::logging::log_line!("configuration illisible, mise de côté : {error}");
                let _ = std::fs::rename(&path, path.with_extension("json.illisible"));
                Config::default()
            }),
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let text = serde_json::to_vec_pretty(self).context("sérialisation de la configuration")?;
        paths::write_atomic(&Self::path()?, &text)
    }

    /// Ramène les valeurs dans des bornes sûres et vide les champs blancs :
    /// la page valide déjà la saisie, mais c'est ici que la règle s'applique.
    pub fn normalized(mut self) -> Self {
        fn blank_to_none(value: &mut Option<String>) {
            if value.as_deref().is_some_and(|text| text.trim().is_empty()) {
                *value = None;
            } else if let Some(text) = value {
                *text = text.trim().to_string();
            }
        }

        self.schema = SCHEMA;
        self.base_url = self.base_url.trim().to_string();
        self.hotkey = self.hotkey.trim().to_string();
        self.translate_hotkey = self.translate_hotkey.trim().to_string();
        self.translate_target = self.translate_target.trim().to_string();
        if self.translate_target.is_empty() {
            self.translate_target = DEFAULT_TRANSLATE_TARGET.to_string();
        }
        self.window_title_match = self.window_title_match.trim().to_string();
        blank_to_none(&mut self.edge_path);
        blank_to_none(&mut self.app_id);
        blank_to_none(&mut self.profile_directory);
        blank_to_none(&mut self.voice);
        blank_to_none(&mut self.lang);
        self.rate = self.rate.map(|value| value.clamp(0.1, 10.0));
        self.pitch = self.pitch.map(|value| value.clamp(0.0, 2.0));
        self.volume = self.volume.map(|value| value.clamp(0.0, 1.0));
        self.max_chars = self.max_chars.min(MAX_CHARS_LIMIT);
        self.activation_delay_ms = self.activation_delay_ms.min(10_000);
        self
    }

    /// Les réglages pour lire un texte en `language` : la voix et la langue
    /// configurées ne valent que si elles parlent déjà cette langue, sinon
    /// c'est à la page de choisir une voix adaptée.
    pub fn speaking(&self, language: &str) -> Config {
        let primary = |code: &str| code.split('-').next().unwrap_or_default().to_lowercase();
        let already = self
            .lang
            .as_deref()
            .is_some_and(|lang| primary(lang) == primary(language));

        if already {
            return self.clone();
        }
        Config {
            voice: None,
            lang: Some(language.to_string()),
            ..self.clone()
        }
    }

    /// Prépare le texte du presse-papiers avant envoi.
    pub fn prepare_text(&self, text: &str) -> String {
        let mut result = text.replace('\u{0}', " ");

        if self.dehyphenate {
            // Mot coupé en fin de ligne : supprimer le tiret et la coupure.
            let mut out = String::with_capacity(result.len());
            let chars: Vec<char> = result.chars().collect();
            let mut index = 0;

            while index < chars.len() {
                let current = chars[index];
                if (current == '-' || current == '\u{ad}')
                    && matches!(chars.get(index + 1), Some('\r' | '\n'))
                {
                    index += 1;
                    while matches!(chars.get(index), Some('\r' | '\n' | ' ')) {
                        index += 1;
                    }
                    continue;
                }

                out.push(current);
                index += 1;
            }

            result = out;
        }

        if self.collapse_whitespace {
            result = result.split_whitespace().collect::<Vec<_>>().join(" ");
        } else {
            result = result.trim().to_string();
        }

        if self.max_chars > 0 && result.chars().count() > self.max_chars {
            result = result.chars().take(self.max_chars).collect();
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_keep_defaults() {
        let config: Config = serde_json::from_str(r#"{"hotkey":"Ctrl+Shift+F9"}"#).unwrap();
        assert_eq!(config.hotkey, "Ctrl+Shift+F9");
        assert!(config.autoplay);
        assert_eq!(config.launch_mode, LaunchMode::App);
    }

    #[test]
    fn collapses_whitespace_and_truncates() {
        let config = Config {
            max_chars: 10,
            ..Config::default()
        };
        assert_eq!(config.prepare_text("  a \n\n b  "), "a b");
        assert_eq!(config.prepare_text("abcdefghijklmnop").chars().count(), 10);
    }

    #[test]
    fn dehyphenates_when_enabled() {
        let config = Config {
            dehyphenate: true,
            ..Config::default()
        };
        assert_eq!(config.prepare_text("exem-\nple simple"), "exemple simple");
        assert_eq!(config.prepare_text("bien-être"), "bien-être");
    }

    #[test]
    fn older_files_keep_their_schema_until_normalized() {
        let config: Config = serde_json::from_str(r#"{"hotkey":"Ctrl+Alt+S"}"#).unwrap();
        assert_eq!(config.schema, 0);
        assert_eq!(config.translate_hotkey, "Ctrl+Alt+T");
        assert_eq!(config.translate_target, "fr");
        assert_eq!(config.normalized().schema, SCHEMA);
    }

    #[test]
    fn a_translation_gets_a_voice_of_its_language() {
        let config = Config {
            voice: Some("Jenny".into()),
            lang: Some("en-US".into()),
            ..Config::default()
        };
        let french = config.speaking("fr");
        assert_eq!(french.voice, None);
        assert_eq!(french.lang.as_deref(), Some("fr"));

        let english = config.speaking("en");
        assert_eq!(english.voice.as_deref(), Some("Jenny"));
        assert_eq!(english.lang.as_deref(), Some("en-US"));
    }

    #[test]
    fn normalizes_blank_fields_and_bounds() {
        let config = Config {
            voice: Some("  ".into()),
            lang: Some(" fr-FR ".into()),
            volume: Some(4.0),
            max_chars: 1_000_000,
            ..Config::default()
        }
        .normalized();
        assert_eq!(config.voice, None);
        assert_eq!(config.lang.as_deref(), Some("fr-FR"));
        assert_eq!(config.volume, Some(1.0));
        assert_eq!(config.max_chars, MAX_CHARS_LIMIT);
    }
}
