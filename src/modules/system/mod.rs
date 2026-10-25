//! Module « Système » : ce qui encombre l'ordinateur, et de quoi y remédier.
//!
//! - **La mémoire** : où elle passe (applications, noyau, le reste), et
//!   quand elle vient à manquer.
//! - **Les processus anormaux** : un processus qui accumule les handles (le
//!   noyau grossit sans qu'aucun gestionnaire de tâches ne l'impute à
//!   personne), une application qui accapare la mémoire, un travail de fond
//!   qui sollicite le processeur sans relâche, une commande restée seule
//!   après la fermeture de ce qui l'avait lancée.
//! - **Les actions** : demander à une application de fermer ses fenêtres,
//!   arrêter un processus, ou ne plus signaler une application.
//!
//! Les processus de Windows et ceux d'Atelier sont protégés : aucune action
//! ne les vise.

mod analysis;
mod config;
mod monitor;
mod probe;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use serde_json::{Value, json};

use self::analysis::{Context, Report, Snapshot, Suspect, Target, Tone};
use self::config::Config;
use self::monitor::Monitor;
use super::{Module, ModuleContext};
use crate::ipc::{HostSender, Notice, NoticeAction, Responder};
use crate::logging::log_line;

pub const ID: &str = "system";

/// Intervalle de la surveillance de fond, qui prévient d'un processus
/// devenu anormal même quand aucune page n'est ouverte.
const WATCH_EVERY: Duration = Duration::from_secs(10);

/// Applications affichées dans la vue d'ensemble.
const TOP_GROUPS: usize = 8;

fn data_dir() -> Result<PathBuf> {
    crate::paths::module_dir(ID)
}

/// Relevé fixe, s'il existe un `demo.json` dans le dossier du module.
fn load_demo() -> Option<Snapshot> {
    let path = data_dir().ok()?.join("demo.json");
    let text = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&text) {
        Ok(snapshot) => {
            log_line!("module Système en mode démonstration : {}", path.display());
            Some(snapshot)
        }
        Err(error) => {
            log_line!("relevé de démonstration illisible : {error}");
            None
        }
    }
}

/// Message d'erreur en phrase : majuscule initiale, point final.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

struct State {
    monitor: Monitor,
    config: Config,
    /// Notifications déjà montrées, pour ne pas répéter la même.
    notified: HashSet<String>,
}

impl State {
    fn report(&mut self) -> Report {
        let snapshot = self.monitor.snapshot();
        analysis::analyze(
            &snapshot,
            &Context {
                now_ms: now_ms(),
                own_pid: std::process::id(),
                ignored: &self.config.ignored,
            },
        )
    }

    fn fresh_report(&mut self) -> Report {
        self.monitor.invalidate();
        self.report()
    }

    fn ensure_real(&self) -> Result<()> {
        if self.monitor.is_demo() {
            Err(anyhow!(
                "mode démonstration\u{a0}: aucun processus réel n’est visé"
            ))
        } else {
            Ok(())
        }
    }

    fn ignored(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.config.ignored.iter().cloned().collect();
        keys.sort();
        keys
    }

    /// Arrête les processus visés, sauf ceux qui sont protégés ou déjà
    /// terminés. Renvoie le nombre d'arrêts et les erreurs rencontrées.
    fn terminate(&mut self, targets: &[Target]) -> Result<(usize, Vec<String>)> {
        self.ensure_real()?;
        let report = self.fresh_report();
        let mut stopped = 0;
        let mut errors: Vec<String> = Vec::new();

        for target in targets {
            let found = report.groups.iter().find_map(|group| {
                group
                    .processes
                    .iter()
                    .find(|entry| entry.pid == target.pid && entry.started == target.started)
                    .map(|entry| (group.name.as_str(), entry))
            });
            let result = match found {
                None => Err(anyhow!("ce processus s’est déjà terminé")),
                Some((_, entry)) if entry.protected => Err(anyhow!(
                    "processus protégé\u{a0}: il appartient à Windows ou à Atelier"
                )),
                Some((name, _)) => probe::terminate(target.pid, target.started).map(|()| {
                    log_line!("processus arrêté : {name} ({})", target.pid);
                }),
            };
            match result {
                Ok(()) => stopped += 1,
                Err(error) => {
                    let message = format!("{error:#}");
                    if !errors.contains(&message) {
                        errors.push(message);
                    }
                }
            }
        }

        self.monitor.invalidate();
        if stopped == 0
            && let Some(first) = errors.first()
        {
            return Err(anyhow!("{first}"));
        }
        Ok((stopped, errors))
    }
}

fn param<'a>(params: &'a Value, name: &str) -> Result<&'a Value> {
    params
        .get(name)
        .filter(|value| !value.is_null())
        .ok_or_else(|| anyhow!("paramètre manquant\u{a0}: {name}"))
}

fn key_param(params: &Value) -> Result<String> {
    param(params, "key")?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("application non précisée"))
}

fn dispatch(state: &Mutex<State>, method: &str, params: Value) -> Result<Value> {
    let mut state = state.lock().unwrap();

    match method {
        "overview" => {
            let mut report = state.report();
            report.groups.truncate(TOP_GROUPS);
            for group in &mut report.groups {
                group.processes.clear();
            }
            Ok(json!({
                "memory": report.memory,
                "breakdown": report.breakdown,
                "alerts": report.alerts,
                "suspects": report.suspects,
                "top": report.groups,
                "ignored": state.ignored(),
                "notify": state.config.notify,
                "demo": state.monitor.is_demo(),
            }))
        }

        "processes" => {
            let report = state.report();
            Ok(json!({
                "memory": report.memory,
                "breakdown": report.breakdown,
                "groups": report.groups,
                "ignored": state.ignored(),
                "demo": state.monitor.is_demo(),
            }))
        }

        "terminate" => {
            let targets: Vec<Target> = serde_json::from_value(param(&params, "targets")?.clone())
                .context("processus visés illisibles")?;
            let (stopped, errors) = state.terminate(&targets)?;
            Ok(json!({ "stopped": stopped, "errors": errors }))
        }

        "close" => {
            state.ensure_real()?;
            let target: Target = serde_json::from_value(param(&params, "target")?.clone())
                .context("processus visé illisible")?;
            let report = state.fresh_report();
            let alive = report.groups.iter().any(|group| {
                group
                    .processes
                    .iter()
                    .any(|entry| entry.pid == target.pid && entry.started == target.started)
            });
            if !alive {
                return Err(anyhow!("ce processus s’est déjà terminé"));
            }
            let windows = probe::close_windows(target.pid);
            if windows == 0 {
                return Err(anyhow!("ce processus n’a aucune fenêtre à fermer"));
            }
            Ok(json!({ "windows": windows }))
        }

        "ignore" => {
            let key = key_param(&params)?;
            let ignored = params
                .get("ignored")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if ignored {
                state.config.ignored.insert(key.clone());
            } else {
                state.config.ignored.remove(&key);
            }
            state.config.save()?;
            log_line!(
                "{key} {} par le module Système",
                if ignored {
                    "ignorée"
                } else {
                    "de nouveau surveillée"
                }
            );
            Ok(json!(state.ignored()))
        }

        "settings.save" => {
            if let Some(notify) = params.get("notify").and_then(Value::as_bool) {
                state.config.notify = notify;
            }
            state.config.save()?;
            Ok(json!({ "notify": state.config.notify }))
        }

        "reveal" => {
            let key = key_param(&params)?;
            let report = state.report();
            let path = report
                .groups
                .iter()
                .find(|group| group.key == key)
                .and_then(|group| group.path.clone())
                .ok_or_else(|| anyhow!("emplacement inconnu"))?;
            let folder = Path::new(&path)
                .parent()
                .ok_or_else(|| anyhow!("emplacement inconnu"))?;
            crate::shell::open(folder)?;
            Ok(Value::Null)
        }

        other => Err(anyhow!("méthode inconnue : {other}")),
    }
}

/// Notification pour un processus qui le mérite (`Suspect::deserves_notice`).
fn notice(suspect: &Suspect) -> Notice {
    let critical = suspect
        .reasons
        .iter()
        .find(|reason| reason.tone == Tone::Critical)
        .map(|reason| reason.kind);
    let (glyph, title, body) = match critical {
        Some("handles") => (
            "alert",
            "Un processus accumule les handles".to_string(),
            format!(
                "{} garde {}{} handles ouverts. La mémoire du noyau qu’ils occupent n’est rendue qu’à son arrêt.",
                suspect.name,
                analysis::format_count(suspect.handles),
                if suspect.handles >= 1_000_000 {
                    " de"
                } else {
                    ""
                }
            ),
        ),
        _ => (
            "memory",
            "Un processus accapare la mémoire".to_string(),
            format!(
                "{} occupe {} sans aucune fenêtre ouverte.",
                suspect.name,
                analysis::format_bytes(suspect.private_bytes)
            ),
        ),
    };

    Notice {
        id: format!("{ID}:{}", suspect.id),
        module: ID.to_string(),
        title,
        body,
        icon: None,
        glyph: Some(glyph),
        tone: "critical",
        countdown: None,
        timeout: None,
        actions: vec![
            NoticeAction::new("dismiss", "Plus tard", false),
            NoticeAction::new("terminate", "Arrêter", true),
        ],
    }
}

/// Surveillance de fond : un relevé, et une notification pour chaque
/// processus devenu anormal depuis le précédent.
fn watch(state: &Mutex<State>, host: &HostSender) {
    let mut state = state.lock().unwrap();
    if state.monitor.is_demo() {
        return;
    }

    let report = state.report();
    let current: HashSet<&str> = report
        .suspects
        .iter()
        .map(|suspect| suspect.id.as_str())
        .collect();
    // Un processus redevenu normal, puis de nouveau anormal, sera signalé
    // une nouvelle fois.
    state.notified.retain(|id| current.contains(id.as_str()));

    if !state.config.notify {
        return;
    }
    for suspect in report
        .suspects
        .iter()
        .filter(|suspect| suspect.deserves_notice())
    {
        if state.notified.insert(suspect.id.clone()) {
            log_line!(
                "processus anormal signalé : {} ({})",
                suspect.name,
                suspect.id
            );
            host.notify(notice(suspect));
        }
    }
}

/// Processus visés par l'identifiant d'un `Suspect` : `pid:<numéro>:<lancement>`
/// ou `app:<clé>`.
fn targets_of(state: &mut State, id: &str) -> Vec<Target> {
    if let Some(rest) = id.strip_prefix("pid:") {
        let mut parts = rest.splitn(2, ':');
        let pid = parts.next().and_then(|text| text.parse().ok());
        let started = parts
            .next()
            .and_then(|text| text.parse::<i64>().ok())
            .filter(|started| *started != 0);
        return pid
            .map(|pid| vec![Target { pid, started }])
            .unwrap_or_default();
    }

    let Some(key) = id.strip_prefix("app:") else {
        return Vec::new();
    };
    state
        .report()
        .groups
        .into_iter()
        .filter(|group| group.key == key)
        .flat_map(|group| group.processes)
        .filter(|entry| !entry.protected)
        .map(|entry| Target {
            pid: entry.pid,
            started: entry.started,
        })
        .collect()
}

pub struct System {
    state: Arc<Mutex<State>>,
    host: Mutex<Option<HostSender>>,
    stop: Mutex<Option<Sender<()>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl System {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                monitor: Monitor::new(load_demo()),
                config: Config::load(),
                notified: HashSet::new(),
            })),
            host: Mutex::new(None),
            stop: Mutex::new(None),
            thread: Mutex::new(None),
        }
    }
}

impl Module for System {
    fn id(&self) -> &'static str {
        ID
    }

    fn script(&self) -> &'static str {
        include_str!("../../../ui/modules/system.js")
    }

    fn default_widgets(&self) -> Vec<(&'static str, &'static str)> {
        vec![("system.memory", "m"), ("system.clutter", "m")]
    }

    fn start(&self, context: ModuleContext) {
        *self.host.lock().unwrap() = Some(context.host.clone());
        let (sender, receiver) = channel::<()>();
        *self.stop.lock().unwrap() = Some(sender);

        let state = self.state.clone();
        let host = context.host;
        let handle = std::thread::Builder::new()
            .name("system".into())
            .spawn(move || {
                // Tout message, ou la fermeture du canal, arrête la boucle.
                while let Err(RecvTimeoutError::Timeout) = receiver.recv_timeout(WATCH_EVERY) {
                    watch(&state, &host);
                }
            });

        match handle {
            Ok(handle) => *self.thread.lock().unwrap() = Some(handle),
            Err(error) => log_line!("surveillance du système non démarrée : {error}"),
        }
    }

    fn handle(&self, method: &str, params: Value, reply: Responder) {
        // Un relevé interroge chaque processus : il se fait hors du fil de
        // l'interface.
        let state = self.state.clone();
        let method = method.to_string();
        std::thread::spawn(move || reply.result(dispatch(&state, &method, params)));
    }

    fn notice_action(&self, notice: &str, action: &str) {
        let Some(id) = notice.strip_prefix(&format!("{ID}:")) else {
            return;
        };
        if action != "terminate" {
            return;
        }

        let state = self.state.clone();
        let host = self.host.lock().unwrap().clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            let mut state = state.lock().unwrap();
            let targets = targets_of(&mut state, &id);
            if let Err(error) = state.terminate(&targets)
                && let Some(host) = host
            {
                host.notify(Notice {
                    id: format!("{ID}:failed"),
                    module: ID.to_string(),
                    title: "Arrêt impossible".to_string(),
                    body: sentence(&format!("{error:#}")),
                    icon: None,
                    glyph: Some("alert"),
                    tone: "warning",
                    countdown: None,
                    timeout: Some(15),
                    actions: vec![NoticeAction::new("dismiss", "Compris", true)],
                });
            }
        });
    }

    fn stop(&self) {
        // Fermer le canal suffit à arrêter la boucle de surveillance.
        self.stop.lock().unwrap().take();
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}
