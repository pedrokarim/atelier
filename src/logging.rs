//! Journal minimal : le binaire n'ayant pas de console en mode fenêtré, les
//! événements notables sont tracés dans `screen-time.log`.

use std::fs::{self, OpenOptions};
use std::io::Write;

use crate::paths;

/// Au-delà de cette taille, le journal est réinitialisé au démarrage.
const MAX_LOG_BYTES: u64 = 256 * 1024;

pub fn rotate_if_needed() {
    if let Ok(path) = paths::log_path()
        && let Ok(metadata) = fs::metadata(&path)
        && metadata.len() > MAX_LOG_BYTES
    {
        let _ = fs::remove_file(&path);
    }
}

/// Écrit une ligne dans le journal, et sur la sortie d'erreur si une console
/// est attachée (lancement depuis un terminal).
pub fn log(message: &str) {
    let line = format!(
        "[{}] {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        message
    );
    eprintln!("{line}");

    let Ok(path) = paths::log_path() else { return };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{line}");
    }
}

macro_rules! log_line {
    ($($arg:tt)*) => { $crate::logging::log(&format!($($arg)*)) };
}

pub(crate) use log_line;
