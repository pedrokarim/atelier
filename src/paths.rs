//! Emplacements des données.
//!
//! ```text
//! %APPDATA%\atelier\
//!   layout.json               widgets de l'accueil et du bureau
//!   atelier.log               journal
//!   webview\                  profil du moteur de rendu
//!   modules\<module>\         données propres à chaque module
//! ```
//!
//! Mode portable : si un dossier `data` existe à côté de l'exécutable, tout y
//! est rangé à la place, et le programme peut voyager sur une clé USB.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow};

const FOLDER_NAME: &str = "atelier";

fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join("data");
    candidate.is_dir().then_some(candidate)
}

/// Dossier racine des données, résolu une seule fois.
pub fn data_dir() -> Result<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

    DIR.get_or_init(|| {
        portable_dir().or_else(|| dirs::data_dir().map(|base| base.join(FOLDER_NAME)))
    })
    .clone()
    .ok_or_else(|| anyhow!("dossier des données introuvable"))
}

pub fn is_portable() -> bool {
    portable_dir().is_some()
}

/// Dossier réservé à un module.
pub fn module_dir(module: &str) -> Result<PathBuf> {
    Ok(data_dir()?.join("modules").join(module))
}

pub fn layout_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("layout.json"))
}

pub fn log_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("atelier.log"))
}

pub fn webview_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("webview"))
}

/// Écrit un fichier en deux temps : un fichier temporaire, puis un renommage.
///
/// Une coupure de courant au milieu de l'écriture laisse ainsi l'ancienne
/// version intacte plutôt qu'un fichier tronqué.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("création de {}", parent.display()))?;
    }

    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, contents)
        .with_context(|| format!("écriture de {}", temporary.display()))?;
    std::fs::rename(&temporary, path)
        .with_context(|| format!("remplacement de {}", path.display()))?;

    Ok(())
}
