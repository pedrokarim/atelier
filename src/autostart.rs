//! Démarrage automatique via `HKCU\...\CurrentVersion\Run`.
//!
//! Essentiel pour les modules qui observent en continu, comme Temps d'écran :
//! le temps passé avant le lancement du programme n'est jamais compté.

use anyhow::{Context, Result};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "atelier";

/// Commande inscrite au démarrage : la copie installée si elle existe.
fn command() -> Result<String> {
    let path = crate::install::preferred_executable()?;
    Ok(format!("\"{}\"", path.display()))
}

pub fn registered_target() -> Option<String> {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_READ)
        .and_then(|key| key.get_value::<String, _>(VALUE_NAME))
        .ok()
}

pub fn is_enabled() -> bool {
    registered_target().is_some()
}

pub fn enable() -> Result<()> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_WRITE)
        .context("ouverture de la clé de démarrage")?;

    key.set_value(VALUE_NAME, &command()?)
        .context("écriture de la valeur de démarrage")?;

    Ok(())
}

pub fn disable() -> Result<()> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_WRITE)
        .context("ouverture de la clé de démarrage")?;

    match key.delete_value(VALUE_NAME) {
        Ok(()) => Ok(()),
        // Absent : l'état voulu est déjà atteint.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("suppression de la valeur de démarrage"),
    }
}

pub fn set(wanted: bool) -> Result<()> {
    if wanted { enable() } else { disable() }
}
