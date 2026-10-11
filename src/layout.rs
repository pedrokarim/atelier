//! Disposition des widgets : la grille de l'accueil et les widgets épinglés
//! sur le bureau, enregistrées dans `layout.json`.
//!
//! L'hôte ne sait rien de ce qu'affiche un widget : il range des instances
//! (un identifiant de widget, une taille, des réglages libres) et des
//! positions. Le rendu appartient au module.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths;

/// Tailles de widget, en cases de la grille.
pub const SIZES: &[(&str, i32, i32)] = &[("s", 1, 1), ("m", 2, 1), ("l", 2, 2), ("w", 4, 1)];

/// Côté d'une case et espacement, en pixels logiques. La grille de l'accueil
/// et les fenêtres du bureau utilisent les mêmes mesures : un widget a la
/// même allure aux deux endroits.
pub const CELL: i32 = 172;
pub const GAP: i32 = 14;

/// Taille logique d'un widget sur le bureau.
pub fn pixel_size(size: &str) -> (i32, i32) {
    let (columns, rows) = SIZES
        .iter()
        .find(|(name, _, _)| *name == size)
        .map(|(_, columns, rows)| (*columns, *rows))
        .unwrap_or((1, 1));
    (
        columns * CELL + (columns - 1) * GAP,
        rows * CELL + (rows - 1) * GAP,
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    /// Identifiant unique de l'instance : un même widget peut être posé
    /// plusieurs fois, avec des réglages différents.
    pub id: String,
    /// Identifiant du widget, préfixé par son module : `screen-time.today`.
    pub widget: String,
    pub size: String,
    #[serde(default)]
    pub settings: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pinned {
    #[serde(flatten)]
    pub instance: Instance,
    /// Position en pixels physiques.
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub topmost: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub board: Vec<Instance>,
    pub desktop: Vec<Pinned>,
    /// Les widgets du bureau sont-ils affichés ? (bascule de la zone de
    /// notification)
    pub desktop_hidden: bool,
    /// Faux tant que la disposition par défaut n'a pas été posée.
    pub initialized: bool,
}

impl Layout {
    pub fn load() -> Layout {
        paths::layout_path()
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let text = serde_json::to_vec_pretty(self).context("sérialisation de la disposition")?;
        paths::write_atomic(&paths::layout_path()?, &text)
    }

    pub fn pinned_mut(&mut self, id: &str) -> Option<&mut Pinned> {
        self.desktop
            .iter_mut()
            .find(|pinned| pinned.instance.id == id)
    }
}

/// Identifiant d'instance neuf.
pub fn new_instance_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("w{now:x}{:x}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_span_cells_and_gaps() {
        assert_eq!(pixel_size("s"), (CELL, CELL));
        assert_eq!(pixel_size("m"), (2 * CELL + GAP, CELL));
        assert_eq!(pixel_size("l"), (2 * CELL + GAP, 2 * CELL + GAP));
        assert_eq!(pixel_size("inconnue"), (CELL, CELL));
    }

    #[test]
    fn pinned_widgets_flatten_their_instance() {
        let pinned = Pinned {
            instance: Instance {
                id: "w1".into(),
                widget: "screen-time.now".into(),
                size: "s".into(),
                settings: Value::Null,
            },
            x: 10,
            y: 20,
            topmost: false,
        };
        let text = serde_json::to_string(&pinned).unwrap();
        assert!(text.contains(r#""widget":"screen-time.now""#));
        let back: Pinned = serde_json::from_str(&text).unwrap();
        assert_eq!(back.instance.id, "w1");
    }
}
