//! Raccourci clavier global.
//!
//! `RegisterHotKey` attache le raccourci au fil qui l'enregistre : c'est
//! dans sa file de messages que Windows dépose `WM_HOTKEY`. Le raccourci a
//! donc son propre fil, avec sa propre boucle de messages, et reçoit ses
//! ordres (changer de combinaison, se suspendre) par un canal doublé d'un
//! message de réveil.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Result, anyhow};
use serde::Serialize;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, KillTimer, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW,
    SetTimer, WM_APP, WM_HOTKEY, WM_QUIT, WM_TIMER,
};

use crate::logging::log_line;

const HOTKEY_ID: i32 = 1;
/// Des ordres attendent dans le canal.
const WM_COMMAND_PENDING: u32 = WM_APP + 1;
/// Une suspension oubliée (page fermée en pleine capture) ne doit pas priver
/// l'utilisateur de son raccourci : elle expire d'elle-même.
const SUSPEND_LIMIT: Duration = Duration::from_secs(60);

/// Raccourci résolu en valeurs Win32.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub modifiers: HOT_KEY_MODIFIERS,
    pub key: u32,
}

/// Convertit « Ctrl+Alt+S » en modificateurs et code de touche virtuelle.
pub fn parse(spec: &str) -> Result<Parsed> {
    let mut modifiers = MOD_NOREPEAT;
    let mut key: Option<u32> = None;

    for part in spec.split('+') {
        let token = part.trim();
        if token.is_empty() {
            continue;
        }

        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "ctl" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" | "maj" => modifiers |= MOD_SHIFT,
            "win" | "super" | "meta" | "cmd" => modifiers |= MOD_WIN,
            other => {
                if key.is_some() {
                    return Err(anyhow!(
                        "«\u{a0}{spec}\u{a0}»\u{a0}: deux touches principales ({other})"
                    ));
                }
                key = Some(parse_key(other).ok_or_else(|| {
                    anyhow!("«\u{a0}{spec}\u{a0}»\u{a0}: touche «\u{a0}{other}\u{a0}» inconnue")
                })?);
            }
        }
    }

    let key = key.ok_or_else(|| anyhow!("«\u{a0}{spec}\u{a0}»\u{a0}: aucune touche principale"))?;

    if modifiers == MOD_NOREPEAT {
        return Err(anyhow!(
            "«\u{a0}{spec}\u{a0}»\u{a0}: au moins un modificateur (Ctrl, Alt, Shift, Win) est requis"
        ));
    }

    Ok(Parsed { modifiers, key })
}

fn parse_key(token: &str) -> Option<u32> {
    // Lettres et chiffres : le code de touche virtuelle est le code ASCII.
    if token.len() == 1 {
        let character = token.chars().next()?;
        if character.is_ascii_alphanumeric() {
            return Some(character.to_ascii_uppercase() as u32);
        }
    }

    // Touches de fonction F1 à F24 (VK_F1 = 0x70).
    if let Some(rest) = token.strip_prefix('f')
        && let Ok(number) = rest.parse::<u32>()
        && (1..=24).contains(&number)
    {
        return Some(0x6F + number);
    }

    if let Some(rest) = token.strip_prefix("num")
        && let Ok(number) = rest.parse::<u32>()
        && number <= 9
    {
        return Some(0x60 + number);
    }

    Some(match token {
        "space" | "espace" => 0x20,
        "enter" | "return" | "entree" | "entrée" => 0x0D,
        "tab" => 0x09,
        "backspace" | "retour" => 0x08,
        "escape" | "esc" | "echap" => 0x1B,
        "insert" | "inser" => 0x2D,
        "delete" | "del" | "suppr" => 0x2E,
        "home" | "origine" => 0x24,
        "end" | "fin" => 0x23,
        "pageup" | "prior" => 0x21,
        "pagedown" | "next" => 0x22,
        "left" | "gauche" => 0x25,
        "up" | "haut" => 0x26,
        "right" | "droite" => 0x27,
        "down" | "bas" => 0x28,
        "pause" => 0x13,
        "printscreen" | "impr" => 0x2C,
        _ => return None,
    })
}

/// Où en est le raccourci, tel que la page l'affiche.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    /// Combinaison effectivement enregistrée auprès de Windows.
    pub active: Option<String>,
    /// Combinaison demandée et refusée, avec la raison.
    pub refused: Option<String>,
    pub error: Option<String>,
    /// Libéré le temps qu'une page capture une nouvelle combinaison.
    pub suspended: bool,
}

enum Command {
    /// Enregistre une combinaison (vide : aucune) et répond si Windows l'a
    /// acceptée. Refusée, la précédente est conservée.
    Set(String, Sender<Result<(), String>>),
    Suspend,
    Resume,
}

pub struct Hotkey {
    thread_id: u32,
    commands: Sender<Command>,
    status: Arc<Mutex<Status>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Hotkey {
    /// Démarre le fil du raccourci. `on_press` est appelé depuis ce fil : il
    /// doit rendre la main vite.
    pub fn start(on_press: impl Fn() + Send + 'static) -> Result<Hotkey> {
        let (commands, receiver) = channel();
        let (ready, started) = channel();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = status.clone();

        let thread = std::thread::Builder::new()
            .name("speech-hotkey".to_string())
            .spawn(move || {
                let mut message = MSG::default();
                // Crée la file de messages du fil avant d'annoncer qu'il est
                // prêt : un message posté plus tôt serait perdu.
                unsafe {
                    let _ = PeekMessageW(&mut message, None, WM_APP, WM_APP, PM_NOREMOVE);
                }
                let _ = ready.send(unsafe { GetCurrentThreadId() });
                run(receiver, shared, on_press);
            })?;

        let thread_id = started
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| anyhow!("le fil du raccourci n’a pas démarré"))?;

        Ok(Hotkey {
            thread_id,
            commands,
            status,
            thread: Mutex::new(Some(thread)),
        })
    }

    fn send(&self, command: Command) {
        if self.commands.send(command).is_ok() {
            unsafe {
                let _ =
                    PostThreadMessageW(self.thread_id, WM_COMMAND_PENDING, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn set(&self, spec: &str) -> Result<(), String> {
        let (reply, answer) = channel();
        self.send(Command::Set(spec.to_string(), reply));
        answer
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| Err("le fil du raccourci ne répond pas".to_string()))
    }

    pub fn suspend(&self) {
        self.send(Command::Suspend);
    }

    pub fn resume(&self) {
        self.send(Command::Resume);
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn stop(&self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
}

fn register(spec: &str) -> Result<(), String> {
    let parsed = parse(spec).map_err(|error| format!("{error:#}"))?;
    unsafe { RegisterHotKey(None, HOTKEY_ID, parsed.modifiers, parsed.key) }.map_err(|_| {
        format!(
            "«\u{a0}{spec}\u{a0}» est déjà utilisé par une autre application, ou réservé par Windows"
        )
    })
}

fn unregister() {
    unsafe {
        let _ = UnregisterHotKey(None, HOTKEY_ID);
    }
}

fn run(commands: Receiver<Command>, status: Arc<Mutex<Status>>, on_press: impl Fn()) {
    // Combinaison voulue, enregistrée ou non selon la suspension.
    let mut wanted: Option<String> = None;
    let mut suspended = false;
    let mut timer = 0usize;
    let mut message = MSG::default();

    loop {
        if unsafe { GetMessageW(&mut message, None, 0, 0) }.0 <= 0 {
            break;
        }

        match message.message {
            WM_HOTKEY if message.wParam.0 as i32 == HOTKEY_ID => on_press(),

            WM_TIMER if message.wParam.0 == timer && timer != 0 => {
                unsafe {
                    let _ = KillTimer(None, timer);
                }
                timer = 0;
                if suspended {
                    suspended = false;
                    if let Some(spec) = &wanted
                        && let Err(error) = register(spec)
                    {
                        log_line!("raccourci non rétabli après suspension : {error}");
                    }
                    status.lock().unwrap().suspended = false;
                }
            }

            WM_COMMAND_PENDING => {
                while let Ok(command) = commands.try_recv() {
                    match command {
                        Command::Set(spec, reply) => {
                            let result = apply(&spec, &mut wanted, suspended, &status);
                            let _ = reply.send(result);
                        }
                        Command::Suspend if !suspended => {
                            suspended = true;
                            unregister();
                            timer = unsafe {
                                SetTimer(None, 0, SUSPEND_LIMIT.as_millis() as u32, None)
                            };
                            status.lock().unwrap().suspended = true;
                        }
                        Command::Resume if suspended => {
                            suspended = false;
                            if timer != 0 {
                                unsafe {
                                    let _ = KillTimer(None, timer);
                                }
                                timer = 0;
                            }
                            if let Some(spec) = &wanted
                                && let Err(error) = register(spec)
                            {
                                log_line!("raccourci non rétabli après suspension : {error}");
                            }
                            status.lock().unwrap().suspended = false;
                        }
                        Command::Suspend | Command::Resume => {}
                    }
                }
            }

            _ => unsafe {
                DispatchMessageW(&message);
            },
        }
    }

    unregister();
}

/// Remplace la combinaison. Refusée, l'ancienne reste en place : mieux vaut
/// un raccourci connu que pas de raccourci du tout.
fn apply(
    spec: &str,
    wanted: &mut Option<String>,
    suspended: bool,
    status: &Mutex<Status>,
) -> Result<(), String> {
    let spec = spec.trim();
    let previous = wanted.clone();

    if spec.is_empty() {
        unregister();
        *wanted = None;
        let mut status = status.lock().unwrap();
        status.active = None;
        status.refused = None;
        status.error = None;
        return Ok(());
    }

    // Pendant une suspension, rien n'est enregistré : la combinaison est
    // seulement validée, et sera posée à la reprise.
    if suspended {
        parse(spec).map_err(|error| format!("{error:#}"))?;
        *wanted = Some(spec.to_string());
        let mut status = status.lock().unwrap();
        status.active = Some(spec.to_string());
        status.refused = None;
        status.error = None;
        return Ok(());
    }

    if previous.as_deref() == Some(spec) && status.lock().unwrap().active.is_some() {
        return Ok(());
    }

    unregister();
    match register(spec) {
        Ok(()) => {
            *wanted = Some(spec.to_string());
            let mut status = status.lock().unwrap();
            status.active = Some(spec.to_string());
            status.refused = None;
            status.error = None;
            log_line!("raccourci de synthèse vocale actif : {spec}");
            Ok(())
        }
        Err(error) => {
            let restored = previous
                .as_deref()
                .filter(|previous| register(previous).is_ok())
                .map(str::to_string);
            let mut status = status.lock().unwrap();
            status.active = restored;
            status.refused = Some(spec.to_string());
            status.error = Some(error.clone());
            // La combinaison voulue reste la nouvelle : la reprise après une
            // suspension, ou un nouvel essai, la retentera.
            *wanted = status.active.clone().or(Some(spec.to_string()));
            log_line!("raccourci de synthèse vocale refusé : {error}");
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_hotkey() {
        let hotkey = parse("Ctrl+Alt+S").unwrap();
        assert_eq!(hotkey.key, 'S' as u32);
        assert!(hotkey.modifiers & MOD_CONTROL == MOD_CONTROL);
        assert!(hotkey.modifiers & MOD_ALT == MOD_ALT);
    }

    #[test]
    fn parses_function_and_numpad_keys() {
        assert_eq!(parse("ctrl+shift+f9").unwrap().key, 0x78);
        assert_eq!(parse("Alt+Num7").unwrap().key, 0x67);
    }

    #[test]
    fn rejects_modifier_only_and_unknown_keys() {
        assert!(parse("Ctrl+Alt").is_err());
        assert!(parse("Ctrl+Alt+Foo").is_err());
        assert!(parse("S").is_err());
        assert!(parse("Ctrl+A+B").is_err());
    }
}
