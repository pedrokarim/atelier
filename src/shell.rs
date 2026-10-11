//! Ouverture de fichiers et de dossiers avec l'application associée.

use std::path::Path;

use anyhow::{Result, anyhow};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

use crate::to_wide;

pub fn open(path: &Path) -> Result<()> {
    let target = to_wide(&path.to_string_lossy());

    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    // Toute valeur supérieure à 32 signale un succès.
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(anyhow!("ouverture de {} impossible", path.display()))
    }
}
