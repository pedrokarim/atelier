//! Lecture du texte du presse-papiers.

use anyhow::{Result, anyhow};
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

/// Texte Unicode, terminé par un zéro.
const CF_UNICODETEXT: u32 = 13;

/// Nombre de tentatives d'ouverture : le presse-papiers est une ressource
/// globale qu'une autre application peut détenir un court instant.
const OPEN_ATTEMPTS: u64 = 10;

/// Ferme le presse-papiers quoi qu'il arrive : resté ouvert, il bloquerait
/// toutes les autres applications.
struct Opened;

impl Drop for Opened {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn read_once() -> Result<String> {
    unsafe { OpenClipboard(None) }.map_err(|_| anyhow!("presse-papiers occupé"))?;
    let _opened = Opened;

    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) }.is_err() {
        return Ok(String::new());
    }

    let handle: HANDLE = unsafe { GetClipboardData(CF_UNICODETEXT) }
        .map_err(|_| anyhow!("presse-papiers illisible"))?;
    let memory = HGLOBAL(handle.0);

    let pointer = unsafe { GlobalLock(memory) } as *const u16;
    if pointer.is_null() {
        return Err(anyhow!("presse-papiers illisible"));
    }

    // Borné par la taille du bloc : un texte sans zéro final ne doit pas
    // faire lire au-delà.
    let capacity = unsafe { GlobalSize(memory) } / 2;
    let units = unsafe { std::slice::from_raw_parts(pointer, capacity) };
    let length = units.iter().position(|unit| *unit == 0).unwrap_or(capacity);
    let text = String::from_utf16_lossy(&units[..length]);

    unsafe {
        let _ = GlobalUnlock(memory);
    }
    Ok(text)
}

/// Lit le texte du presse-papiers, en réessayant s'il est verrouillé. Vide
/// s'il ne contient pas de texte.
pub fn read_text() -> Result<String> {
    let mut last_error = None;

    for attempt in 0..OPEN_ATTEMPTS {
        match read_once() {
            Ok(text) => return Ok(text),
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(20 * (attempt + 1)));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("presse-papiers illisible")))
}
