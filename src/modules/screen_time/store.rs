//! Modèle des données et persistance.
//!
//! Une journée par fichier (`days\AAAA-MM-JJ.json`) : les jours passés ne
//! changent plus, un fichier corrompu ne coûte qu'une journée, et l'historique
//! se lit à la demande sans base de données.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::paths;

/// Secondes depuis minuit, heure locale.
pub type DaySecond = u32;

pub const DAY_SECONDS: DaySecond = 24 * 3600;

/// Utilisation d'une application sur une journée.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppDay {
    /// Secondes au premier plan, utilisateur présent.
    pub active: u64,
    /// Secondes pendant lesquelles au moins un processus était lancé.
    pub open: u64,
    /// Nombre de lancements observés.
    pub launches: u32,
    /// Secondes au premier plan, heure par heure.
    pub hours: [u32; 24],
}

/// Un intervalle continu : au premier plan, ou lancé.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub app: String,
    pub start: DaySecond,
    pub end: DaySecond,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DayRecord {
    pub date: String,
    pub apps: BTreeMap<String, AppDay>,
    /// Intervalles au premier plan, dans l'ordre chronologique.
    pub focus: Vec<Span>,
    /// Sessions d'ouverture, du lancement à la fermeture.
    pub sessions: Vec<Span>,
    /// Minutes accordées en plus du budget, par application.
    pub extensions: BTreeMap<String, u32>,
}

fn day_path(date: NaiveDate) -> Result<PathBuf> {
    Ok(super::data_dir()?
        .join("days")
        .join(format!("{}.json", date.format("%Y-%m-%d"))))
}

impl DayRecord {
    pub fn empty(date: NaiveDate) -> Self {
        Self {
            date: date.format("%Y-%m-%d").to_string(),
            ..Default::default()
        }
    }

    /// Charge une journée ; une journée sans fichier est une journée vide.
    pub fn load(date: NaiveDate) -> Self {
        let Ok(path) = day_path(date) else {
            return Self::empty(date);
        };

        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<DayRecord>(&text).unwrap_or_else(|error| {
                crate::logging::log_line!("{} illisible : {error}", path.display());
                Self::empty(date)
            }),
            Err(_) => Self::empty(date),
        }
    }

    pub fn save(&self) -> Result<()> {
        let date = self.naive_date()?;
        let text = serde_json::to_vec(self).context("sérialisation de la journée")?;
        paths::write_atomic(&day_path(date)?, &text)
    }

    pub fn naive_date(&self) -> Result<NaiveDate> {
        NaiveDate::parse_from_str(&self.date, "%Y-%m-%d")
            .with_context(|| format!("date invalide : {}", self.date))
    }

    pub fn total_active(&self) -> u64 {
        self.apps.values().map(|app| app.active).sum()
    }

    pub fn summary(&self) -> DaySummary {
        DaySummary {
            total_active: self.total_active(),
            apps: self
                .apps
                .iter()
                .map(|(key, day)| {
                    (
                        key.clone(),
                        Totals {
                            active: day.active,
                            open: day.open,
                            launches: day.launches,
                        },
                    )
                })
                .collect(),
        }
    }
}

/// Totaux d'une application, sur une journée ou une période.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Totals {
    pub active: u64,
    pub open: u64,
    pub launches: u32,
}

impl Totals {
    pub fn add(&mut self, other: &Totals) {
        self.active += other.active;
        self.open += other.open;
        self.launches += other.launches;
    }
}

/// Résumé d'une journée : ce dont l'historique a besoin, sans le détail.
#[derive(Debug, Clone, Default)]
pub struct DaySummary {
    pub total_active: u64,
    pub apps: BTreeMap<String, Totals>,
}

/// Résumés des jours passés, lus une fois puis gardés : un jour révolu ne
/// change plus.
#[derive(Default)]
pub struct History {
    cache: HashMap<NaiveDate, DaySummary>,
}

impl History {
    /// Résumé d'un jour passé. Le jour courant se lit dans l'état partagé,
    /// jamais ici.
    pub fn past(&mut self, date: NaiveDate) -> &DaySummary {
        self.cache
            .entry(date)
            .or_insert_with(|| DayRecord::load(date).summary())
    }

    /// Toutes les dates pour lesquelles un fichier existe.
    pub fn recorded_dates() -> Vec<NaiveDate> {
        let Ok(dir) = super::data_dir().map(|dir| dir.join("days")) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };

        let mut dates: Vec<NaiveDate> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let stem = name.strip_suffix(".json")?;
                NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok()
            })
            .collect();
        dates.sort();
        dates
    }
}

/// Ce que l'on sait d'une application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    /// Nom lu dans les métadonnées de l'exécutable.
    pub name: String,
    /// Dernier chemin complet observé.
    pub path: String,
    /// Icône, en URL de données PNG.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Nom choisi par l'utilisateur, prioritaire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_name: Option<String>,
    /// Masquée des statistiques par l'utilisateur.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

impl AppInfo {
    pub fn display_name(&self) -> &str {
        self.custom_name.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub apps: BTreeMap<String, AppInfo>,
    /// Incrémenté à chaque modification : la page ne recharge les icônes que
    /// lorsqu'il change.
    #[serde(skip)]
    pub version: u64,
}

impl Catalog {
    pub fn load() -> Catalog {
        super::data_dir()
            .ok()
            .map(|dir| dir.join("apps.json"))
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<Catalog>(&text).ok())
            .map(|catalog| Catalog {
                version: 1,
                ..catalog
            })
            .unwrap_or(Catalog {
                version: 1,
                ..Default::default()
            })
    }

    pub fn save(&self) -> Result<()> {
        let text = serde_json::to_vec(self).context("sérialisation du catalogue")?;
        paths::write_atomic(&super::data_dir()?.join("apps.json"), &text)
    }

    pub fn name_of(&self, key: &str) -> String {
        self.apps
            .get(key)
            .map(|info| info.display_name().to_string())
            .unwrap_or_else(|| fallback_name(key))
    }

    pub fn is_hidden(&self, key: &str) -> bool {
        self.apps.get(key).is_some_and(|info| info.hidden)
    }
}

/// Nom de repli, quand l'exécutable ne décrit pas son application : le nom
/// du fichier, sans extension, mots séparés (`GenshinImpact.exe` devient
/// « Genshin Impact »).
pub fn fallback_name(path: &str) -> String {
    let file = path.rsplit('\\').next().unwrap_or(path);
    let stem = match file.rfind('.') {
        Some(dot) if file[dot..].eq_ignore_ascii_case(".exe") => &file[..dot],
        _ => file,
    };

    let mut words = String::new();
    let mut previous: Option<char> = None;
    for character in stem.chars() {
        let character = if character == '_' || character == '-' {
            ' '
        } else {
            character
        };
        if let Some(before) = previous
            && before.is_lowercase()
            && character.is_uppercase()
        {
            words.push(' ');
        }
        words.push(character);
        previous = Some(character);
    }

    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => path.to_string(),
    }
}

/// Clé stable d'une application à partir du chemin de son exécutable.
///
/// Le chemin complet distingue deux `launcher.exe` de jeux différents ; les
/// segments de version sont neutralisés pour qu'une mise à jour (Discord,
/// `app-1.0.9…`, ou un paquet du Store, `…_1.21.3_x64__…`) ne crée pas une
/// nouvelle application.
pub fn app_key(path: &str) -> String {
    path.to_lowercase()
        .split('\\')
        .map(|segment| {
            segment
                .split('_')
                .map(|part| if is_version(part) { "*" } else { part })
                .collect::<Vec<_>>()
                .join("_")
        })
        .collect::<Vec<_>>()
        .join("\\")
}

fn is_version(part: &str) -> bool {
    let part = part.strip_prefix("app-").unwrap_or(part);
    part.contains('.')
        && part.starts_with(|c: char| c.is_ascii_digit())
        && part.chars().all(|c| c.is_ascii_digit() || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_segments_are_neutralised() {
        assert_eq!(
            app_key(r"C:\Users\x\AppData\Local\Discord\app-1.0.9187\Discord.exe"),
            r"c:\users\x\appdata\local\discord\*\discord.exe"
        );
        assert_eq!(
            app_key(
                r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.21.3231.0_x64__8wekyb3d8bbwe\WindowsTerminal.exe"
            ),
            r"c:\program files\windowsapps\microsoft.windowsterminal_*_x64__8wekyb3d8bbwe\windowsterminal.exe"
        );
    }

    #[test]
    fn ordinary_paths_are_kept() {
        assert_eq!(
            app_key(r"C:\Program Files\Genshin Impact\Genshin Impact game\GenshinImpact.exe"),
            r"c:\program files\genshin impact\genshin impact game\genshinimpact.exe"
        );
        // Un nom contenant des chiffres n'est pas une version.
        assert_eq!(app_key(r"D:\Games\7zFM.exe"), r"d:\games\7zfm.exe");
    }

    #[test]
    fn fallback_name_separates_words() {
        assert_eq!(
            fallback_name(r"C:\Games\Genshin Impact game\GenshinImpact.exe"),
            "Genshin Impact"
        );
        assert_eq!(fallback_name(r"c:\games\my_tool-x.exe"), "My tool x");
        assert_eq!(
            fallback_name(r"c:\games\genshinimpact.exe"),
            "Genshinimpact"
        );
    }
}
