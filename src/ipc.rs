//! Échanges entre les pages et l'hôte.
//!
//! ```text
//! page    --postMessage {kind:"request", id, target, method, params}-->  hôte
//! hôte    --atelier.resolve(id, ok, value)------------------------------>  page
//! module  --HostEvent (depuis n'importe quel fil)----------------------->  hôte
//! ```
//!
//! Une requête reçoit un `Responder`. Le module peut répondre tout de suite,
//! ou le garder et répondre plus tard depuis un autre fil : c'est ce qui
//! permettra à un widget d'interroger un service réseau (la bourse, la météo)
//! sans figer l'interface.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::Sender;

use serde::Serialize;
use serde_json::Value;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Réveille la boucle de messages : des événements attendent dans le canal.
pub const WM_APP_WAKE: u32 = WM_APP + 1;
/// Une seconde instance demande l'ouverture de la fenêtre principale.
pub const WM_APP_SHOW: u32 = WM_APP + 2;
/// Une fenêtre a été fermée par l'utilisateur ; `lparam` porte son HWND.
pub const WM_APP_WINDOW_CLOSED: u32 = WM_APP + 3;
/// Une fenêtre a été déplacée ; `lparam` porte son HWND.
pub const WM_APP_WINDOW_MOVED: u32 = WM_APP + 4;
/// Le thème de Windows a changé.
pub const WM_APP_THEME: u32 = WM_APP + 5;
/// Battement de la minuterie de l'hôte.
pub const WM_APP_TIMER: u32 = WM_APP + 6;

/// Fenêtre cachée qui reçoit les messages de l'application.
static HOST: AtomicIsize = AtomicIsize::new(0);

pub fn set_host(hwnd: HWND) {
    HOST.store(hwnd.0 as isize, Ordering::SeqCst);
}

pub fn host() -> HWND {
    HWND(HOST.load(Ordering::SeqCst) as *mut _)
}

pub fn post_to_host(message: u32, lparam: isize) {
    let hwnd = host();
    if !hwnd.is_invalid() {
        unsafe {
            let _ = PostMessageW(Some(hwnd), message, WPARAM(0), LPARAM(lparam));
        }
    }
}

pub fn wake() {
    post_to_host(WM_APP_WAKE, 0);
}

/// Identifiant d'une fenêtre web de l'application.
pub type WindowId = u64;

/// Notification affichée dans le coin de l'écran.
#[derive(Debug, Clone, Serialize)]
pub struct Notice {
    /// Une notification de même identifiant remplace la précédente.
    pub id: String,
    pub module: String,
    pub title: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Pictogramme de la bibliothèque d'icônes, affiché quand il n'y a pas
    /// d'image ; à défaut, celui du ton.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glyph: Option<&'static str>,
    /// `info`, `warning` ou `critical`.
    pub tone: &'static str,
    /// Compte à rebours affiché, en secondes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub countdown: Option<u32>,
    /// Fermeture automatique, en secondes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
    pub actions: Vec<NoticeAction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoticeAction {
    pub id: String,
    pub label: String,
    pub primary: bool,
}

impl NoticeAction {
    pub fn new(id: &str, label: &str, primary: bool) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            primary,
        }
    }
}

/// Ce que les modules et les répondeurs envoient à l'hôte.
pub enum HostEvent {
    Reply {
        window: WindowId,
        request: u64,
        result: Result<Value, String>,
    },
    /// Diffusé à toutes les fenêtres : `atelier.emit(event, payload)`.
    Broadcast {
        event: String,
        payload: Value,
    },
    Notice(Notice),
    DismissNotice(String),
}

/// Expédie des événements à l'hôte depuis n'importe quel fil.
#[derive(Clone)]
pub struct HostSender(Sender<HostEvent>);

impl HostSender {
    pub fn new(sender: Sender<HostEvent>) -> Self {
        Self(sender)
    }

    pub fn send(&self, event: HostEvent) {
        if self.0.send(event).is_ok() {
            wake();
        }
    }

    pub fn broadcast(&self, event: &str, payload: Value) {
        self.send(HostEvent::Broadcast {
            event: event.to_string(),
            payload,
        });
    }

    pub fn notify(&self, notice: Notice) {
        self.send(HostEvent::Notice(notice));
    }

    pub fn dismiss(&self, id: &str) {
        self.send(HostEvent::DismissNotice(id.to_string()));
    }
}

/// Réponse due à une requête de page.
///
/// Abandonné sans réponse, il répond par une erreur : une promesse côté page
/// ne doit jamais rester suspendue.
pub struct Responder {
    window: WindowId,
    request: u64,
    sender: Option<HostSender>,
}

impl Responder {
    pub fn new(window: WindowId, request: u64, sender: HostSender) -> Self {
        Self {
            window,
            request,
            sender: Some(sender),
        }
    }

    fn finish(mut self, result: Result<Value, String>) {
        if let Some(sender) = self.sender.take() {
            sender.send(HostEvent::Reply {
                window: self.window,
                request: self.request,
                result,
            });
        }
    }

    pub fn ok<T: Serialize>(self, value: T) {
        let value = serde_json::to_value(value).unwrap_or(Value::Null);
        self.finish(Ok(value));
    }

    pub fn err(self, message: impl Into<String>) {
        self.finish(Err(message.into()));
    }

    /// Répond avec le résultat d'une opération faillible.
    pub fn result<T: Serialize>(self, result: anyhow::Result<T>) {
        match result {
            Ok(value) => self.ok(value),
            Err(error) => self.err(format!("{error:#}")),
        }
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            sender.send(HostEvent::Reply {
                window: self.window,
                request: self.request,
                result: Err("requête restée sans réponse".to_string()),
            });
        }
    }
}

/// Requête émise par une page.
pub struct Request {
    pub id: u64,
    pub target: String,
    pub method: String,
    pub params: Value,
}

pub fn parse_request(raw: &str) -> Option<Request> {
    let value: Value = serde_json::from_str(raw).ok()?;
    if value.get("kind")?.as_str()? != "request" {
        return None;
    }

    Some(Request {
        id: value.get("id")?.as_u64()?,
        target: value.get("target")?.as_str()?.to_string(),
        method: value.get("method")?.as_str()?.to_string(),
        params: value.get("params").cloned().unwrap_or(Value::Null),
    })
}

/// Encode une valeur pour l'insérer dans du script.
pub fn json<T: Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}
