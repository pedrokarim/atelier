//! Réglages du module, enregistrés dans `config.json`.

use std::collections::HashSet;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Prévenir par une notification quand un processus devient anormal.
    pub notify: bool,
    /// Applications (clés) que l'utilisateur ne veut plus voir signalées :
    /// un jeu gourmand, par exemple, l'est par nature.
    pub ignored: HashSet<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            notify: true,
            ignored: HashSet::new(),
        }
    }
}

impl Config {
    /// Un fichier illisible est mis de côté : les valeurs par défaut
    /// s'appliquent.
    pub fn load() -> Config {
        let Ok(path) = super::data_dir().map(|dir| dir.join("config.json")) else {
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
        let path = super::data_dir()?.join("config.json");
        let text = serde_json::to_vec_pretty(self).context("sérialisation de la configuration")?;
        paths::write_atomic(&path, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_keep_defaults() {
        let config: Config = serde_json::from_str(r#"{"ignored":["c:\\jeux\\jeu.exe"]}"#).unwrap();
        assert!(config.notify);
        assert!(config.ignored.contains(r"c:\jeux\jeu.exe"));
    }
}
