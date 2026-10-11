//! Réglages et budgets du module, enregistrés dans `config.json`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths;

/// Mesure à laquelle un budget s'applique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Metric {
    /// Temps passé avec l'application au premier plan, utilisateur présent.
    Active,
    /// Temps pendant lequel l'application est lancée, qu'on la regarde ou non.
    Open,
}

/// Ce qui se passe quand le budget est épuisé.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Prévenir, puis rappeler régulièrement.
    Notify,
    /// Prévenir, laisser un délai de grâce, puis fermer l'application.
    Close,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    /// Clé de l'application dans le catalogue.
    pub app: String,
    /// Durée quotidienne autorisée.
    pub minutes: u32,
    pub metric: Metric,
    pub action: Action,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Sans clavier ni souris depuis cette durée, l'utilisateur est absent.
    pub idle_minutes: u32,
    /// Une application en plein écran (jeu, vidéo) reste comptée comme
    /// utilisée même sans clavier ni souris : une manette ou un film ne
    /// produisent aucune saisie aux yeux de Windows.
    pub fullscreen_counts_as_active: bool,
    /// Premier avertissement, avant l'épuisement d'un budget.
    pub warn_minutes: u32,
    /// Durée accordée par le bouton « + N min » d'une notification.
    pub extension_minutes: u32,
    /// Délai entre l'annonce de la fermeture et la fermeture effective.
    pub close_grace_seconds: u32,
    /// Intervalle des rappels quand un budget « Prévenir » est dépassé.
    pub reminder_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            idle_minutes: 5,
            fullscreen_counts_as_active: true,
            warn_minutes: 5,
            extension_minutes: 15,
            close_grace_seconds: 60,
            reminder_minutes: 30,
        }
    }
}

impl Settings {
    /// Ramène les valeurs saisies dans des bornes raisonnables.
    pub fn clamp(mut self) -> Self {
        self.idle_minutes = self.idle_minutes.clamp(1, 120);
        self.warn_minutes = self.warn_minutes.min(120);
        self.extension_minutes = self.extension_minutes.clamp(1, 240);
        self.close_grace_seconds = self.close_grace_seconds.clamp(10, 600);
        self.reminder_minutes = self.reminder_minutes.clamp(5, 240);
        self
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub settings: Settings,
    pub budgets: Vec<Budget>,
}

impl Config {
    /// Charge la configuration. Un fichier illisible n'empêche pas le suivi :
    /// il est mis de côté et les valeurs par défaut s'appliquent.
    pub fn load() -> Config {
        let Ok(path) = super::data_dir().map(|dir| dir.join("config.json")) else {
            return Config::default();
        };

        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Config>(&text) {
                Ok(config) => Config {
                    settings: config.settings.clamp(),
                    ..config
                },
                Err(error) => {
                    crate::logging::log_line!("configuration illisible, mise de côté : {error}");
                    let _ = std::fs::rename(&path, path.with_extension("json.illisible"));
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = super::data_dir()?.join("config.json");
        let text = serde_json::to_vec_pretty(self).context("sérialisation de la configuration")?;
        paths::write_atomic(&path, &text)
    }

    /// Ajoute le budget, ou remplace celui de la même application.
    pub fn upsert_budget(&mut self, budget: Budget) {
        match self
            .budgets
            .iter_mut()
            .find(|existing| existing.app == budget.app)
        {
            Some(existing) => *existing = budget,
            None => self.budgets.push(budget),
        }
    }

    pub fn remove_budget(&mut self, app: &str) {
        self.budgets.retain(|budget| budget.app != app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_keeps_defaults() {
        let config: Config = serde_json::from_str(r#"{"settings":{"idle_minutes":10}}"#).unwrap();
        assert_eq!(config.settings.idle_minutes, 10);
        assert_eq!(config.settings.extension_minutes, 15);
        assert!(config.budgets.is_empty());
    }

    #[test]
    fn budget_round_trip() {
        let mut config = Config::default();
        config.upsert_budget(Budget {
            app: "genshinimpact.exe".into(),
            minutes: 90,
            metric: Metric::Active,
            action: Action::Close,
            enabled: true,
        });
        config.upsert_budget(Budget {
            app: "genshinimpact.exe".into(),
            minutes: 60,
            metric: Metric::Open,
            action: Action::Notify,
            enabled: true,
        });
        assert_eq!(config.budgets.len(), 1);

        let text = serde_json::to_string(&config).unwrap();
        assert!(text.contains(r#""metric":"open""#));
        let back: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(back.budgets[0].minutes, 60);
    }
}
