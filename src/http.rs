//! Requêtes HTTPS, par WinHTTP : la pile réseau de Windows, avec ses
//! certificats et son proxy. Aucune dépendance de plus dans le binaire.

use std::ffi::c_void;

use anyhow::{Result, anyhow};
use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows::core::PCWSTR;

use crate::to_wide;

/// Délais de résolution, connexion, envoi et réception, en millisecondes.
const TIMEOUT_MS: i32 = 30_000;

/// Poignée WinHTTP fermée quoi qu'il arrive.
struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void, what: &str) -> Result<Handle> {
        if raw.is_null() {
            Err(anyhow!(
                "{what} impossible ({})",
                windows::core::Error::from_thread()
            ))
        } else {
            Ok(Handle(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

/// Sépare `https://hôte/chemin` en hôte et chemin. Seul HTTPS est accepté.
fn split_url(url: &str) -> Result<(&str, &str)> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| anyhow!("adresse non sécurisée refusée : {url}"))?;
    Ok(match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    })
}

/// Télécharge `url` en entier, redirections suivies. Au-delà de `limit`
/// octets, la réponse est refusée.
pub fn get(url: &str, accept: &str, limit: usize) -> Result<Vec<u8>> {
    let (host, path) = split_url(url)?;
    let agent = to_wide(&format!("Atelier/{}", env!("CARGO_PKG_VERSION")));
    let host_wide = to_wide(host);
    let path_wide = to_wide(path);
    let verb = to_wide("GET");
    let headers: Vec<u16> = format!("Accept: {accept}\r\n").encode_utf16().collect();

    unsafe {
        let session = Handle::new(
            WinHttpOpen(
                PCWSTR(agent.as_ptr()),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ),
            "ouverture de la session réseau",
        )?;
        WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS)?;

        let connection = Handle::new(
            WinHttpConnect(
                session.0,
                PCWSTR(host_wide.as_ptr()),
                INTERNET_DEFAULT_HTTPS_PORT,
                0,
            ),
            "connexion",
        )?;

        let request = Handle::new(
            WinHttpOpenRequest(
                connection.0,
                PCWSTR(verb.as_ptr()),
                PCWSTR(path_wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            "préparation de la requête",
        )?;

        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0)
            .map_err(|error| anyhow!("{host} injoignable ({error})"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())
            .map_err(|error| anyhow!("{host} n’a pas répondu ({error})"))?;

        let mut status = 0u32;
        let mut size = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut size,
            std::ptr::null_mut(),
        )?;
        if status != 200 {
            return Err(anyhow!("{host} a répondu {status}"));
        }

        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut available)?;
            if available == 0 {
                break;
            }
            let start = body.len();
            if start + available as usize > limit {
                return Err(anyhow!("réponse trop volumineuse"));
            }
            body.resize(start + available as usize, 0);
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                body[start..].as_mut_ptr() as *mut c_void,
                available,
                &mut read,
            )?;
            body.truncate(start + read as usize);
        }

        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_https_urls_only() {
        assert_eq!(
            split_url("https://api.github.com/repos/a/b").unwrap(),
            ("api.github.com", "/repos/a/b")
        );
        assert_eq!(
            split_url("https://exemple.fr").unwrap(),
            ("exemple.fr", "/")
        );
        assert!(split_url("http://exemple.fr/").is_err());
    }
}
