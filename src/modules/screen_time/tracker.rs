//! Le suivi lui-même : une observation par seconde, sur un fil dédié.
//!
//! À chaque tour :
//!
//! 1. les processus sont recensés et rattachés à une application ;
//! 2. les fenêtres visibles révèlent les nouvelles applications du catalogue ;
//! 3. l'application au premier plan reçoit la seconde, si l'utilisateur est
//!    présent ; toutes les applications lancées reçoivent du temps « ouvert » ;
//! 4. les budgets sont évalués, et les fermetures dues sont exécutées.
//!
//! L'état partagé n'est verrouillé que le temps des écritures : les lectures
//! système (processus, icônes) se font à l'extérieur.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate, Timelike};
use serde::Serialize;

use super::config::{Action, Config, Metric};
use super::probe;
use super::store::{AppInfo, Catalog, DAY_SECONDS, DayRecord, DaySecond, Span, app_key};
use crate::ipc::{HostSender, Notice, NoticeAction};
use crate::logging::log_line;

/// Un trou plus court que ceci ne coupe pas un intervalle : l'observation
/// n'est pas parfaitement régulière.
const SPAN_TOLERANCE: DaySecond = 5;
/// Au-delà, l'écart entre deux tours n'est pas compté : veille, hibernation.
const MAX_TICK_SECONDS: f64 = 3.0;
/// Intervalle d'enregistrement de la journée sur le disque.
const SAVE_EVERY: Duration = Duration::from_secs(60);
/// Délai entre la demande de fermeture polie et la fermeture forcée.
const FORCE_CLOSE_AFTER: Duration = Duration::from_secs(10);

/// Ce que le reste du programme voit de l'instant présent.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Live {
    pub foreground: Option<String>,
    /// Début de l'intervalle au premier plan en cours (secondes du jour).
    pub foreground_since: Option<DaySecond>,
    pub idle: bool,
    pub paused: bool,
    /// Applications lancées, avec l'heure de lancement de leur plus ancien
    /// processus (millisecondes Unix).
    pub running: BTreeMap<String, i64>,
}

pub struct Shared {
    pub day: DayRecord,
    pub catalog: Catalog,
    pub config: Config,
    pub live: Live,
    pub paused: bool,
}

pub type SharedState = Arc<Mutex<Shared>>;

pub enum Command {
    /// Accorder la rallonge réglée à une application.
    Extend(String),
    /// Fermer l'application sans attendre la fin du délai de grâce.
    CloseNow(String),
    Stop,
}

/// Ce que le fil sait d'un processus.
struct ProcessInfo {
    key: Option<String>,
    path: String,
    started_ms: Option<i64>,
}

/// Où en sont les avertissements d'un budget, pour une limite donnée.
#[derive(Default)]
struct BudgetProgress {
    limit: u64,
    warned_soon: bool,
    reached: bool,
    reminders: u64,
}

struct PendingClose {
    deadline: Instant,
    close_sent: Option<Instant>,
}

pub struct Tracker {
    shared: SharedState,
    host: HostSender,
    processes: HashMap<u32, ProcessInfo>,
    open_sessions: HashMap<String, usize>,
    previously_open: HashSet<String>,
    first_tick: bool,
    last_tick: Instant,
    carry: f64,
    last_save: Instant,
    progress: HashMap<String, BudgetProgress>,
    pending: HashMap<String, PendingClose>,
}

impl Tracker {
    pub fn new(shared: SharedState, host: HostSender) -> Self {
        Self {
            shared,
            host,
            processes: HashMap::new(),
            open_sessions: HashMap::new(),
            previously_open: HashSet::new(),
            first_tick: true,
            last_tick: Instant::now(),
            carry: 0.0,
            last_save: Instant::now(),
            progress: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    pub fn run(mut self, commands: Receiver<Command>) {
        log_line!("suivi du temps d'écran démarré");

        loop {
            let wait = Duration::from_secs(1).saturating_sub(self.last_tick.elapsed());
            match commands.recv_timeout(wait) {
                Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(command) => self.apply(command),
                Err(RecvTimeoutError::Timeout) => {}
            }

            if self.last_tick.elapsed() >= Duration::from_millis(950) {
                self.tick();
            }
        }

        self.save();
        log_line!("suivi du temps d'écran arrêté");
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Extend(app) => {
                let mut shared = self.shared.lock().unwrap();
                let minutes = shared.config.settings.extension_minutes;
                *shared.day.extensions.entry(app.clone()).or_default() += minutes;
                drop(shared);

                self.pending.remove(&app);
                self.host.dismiss(&notice_id(&app));
                log_line!("rallonge de {minutes} min accordée à {app}");
            }
            Command::CloseNow(app) => {
                let entry = self.pending.entry(app).or_insert(PendingClose {
                    deadline: Instant::now(),
                    close_sent: None,
                });
                entry.deadline = Instant::now();
            }
            Command::Stop => {}
        }
    }

    pub fn save(&self) {
        let (day, catalog) = {
            let shared = self.shared.lock().unwrap();
            (shared.day.clone(), shared.catalog.clone())
        };
        if let Err(error) = day.save() {
            log_line!("journée non enregistrée : {error:#}");
        }
        if let Err(error) = catalog.save() {
            log_line!("catalogue non enregistré : {error:#}");
        }
    }

    // --- Observation -------------------------------------------------------

    /// Rattache les nouveaux processus à une application et oublie les morts.
    fn refresh_processes(&mut self) {
        let alive: HashSet<u32> = probe::running_processes().into_iter().collect();
        self.processes.retain(|pid, _| alive.contains(pid));

        for pid in alive {
            if self.processes.contains_key(&pid) {
                continue;
            }

            let path = probe::process_path(pid).unwrap_or_default();
            let key =
                (!path.is_empty() && !probe::is_shell_executable(&path)).then(|| app_key(&path));
            let started_ms = key.as_ref().and_then(|_| probe::process_start_ms(pid));

            self.processes.insert(
                pid,
                ProcessInfo {
                    key,
                    path,
                    started_ms,
                },
            );
        }
    }

    /// Ajoute au catalogue les applications encore inconnues.
    ///
    /// Le nom et l'icône sont lus hors verrou : l'extraction d'une icône peut
    /// prendre quelques dizaines de millisecondes.
    fn discover(&mut self, pids: impl IntoIterator<Item = u32>) {
        let candidates: Vec<(String, String)> = pids
            .into_iter()
            .filter_map(|pid| {
                let info = self.processes.get(&pid)?;
                Some((info.key.clone()?, info.path.clone()))
            })
            .collect();

        let unknown: Vec<(String, String)> = {
            let shared = self.shared.lock().unwrap();
            let mut seen = HashSet::new();
            candidates
                .into_iter()
                .filter(|(key, _)| {
                    !shared.catalog.apps.contains_key(key) && seen.insert(key.clone())
                })
                .collect()
        };

        if unknown.is_empty() {
            return;
        }

        let discovered: Vec<(String, AppInfo)> = unknown
            .into_iter()
            .map(|(key, path)| {
                let info = AppInfo {
                    name: probe::executable_name(&path)
                        .unwrap_or_else(|| super::store::fallback_name(&path)),
                    icon: probe::executable_icon(&path),
                    path,
                    custom_name: None,
                    hidden: false,
                };
                (key, info)
            })
            .collect();

        let mut shared = self.shared.lock().unwrap();
        for (key, info) in discovered {
            log_line!("nouvelle application : {} ({})", info.name, info.path);
            shared.catalog.apps.insert(key, info);
        }
        shared.catalog.version += 1;
    }

    fn tick(&mut self) {
        let elapsed = self.last_tick.elapsed().as_secs_f64();
        self.last_tick = Instant::now();
        self.carry += elapsed.min(MAX_TICK_SECONDS);
        let whole = self.carry.floor();
        self.carry -= whole;
        let whole = whole as u64;

        self.refresh_processes();

        let foreground = probe::foreground();
        let mut visible: Vec<u32> = probe::processes_with_windows().into_iter().collect();
        if let Some(window) = &foreground {
            if !self.processes.contains_key(&window.pid) {
                // Processus apparu entre le recensement et la lecture.
                self.refresh_processes();
            }
            visible.push(window.pid);
        }
        self.discover(visible);

        let now = Local::now();
        let date = now.date_naive();
        let second = now.num_seconds_from_midnight();
        let hour = now.hour() as usize;

        // Un clone de l'`Arc` : le verrou ne doit pas emprunter `self`, que
        // l'évaluation des budgets modifie.
        let state = self.shared.clone();
        let mut shared = state.lock().unwrap();

        if shared.day.date != date.format("%Y-%m-%d").to_string() {
            drop(shared);
            self.roll_over(date);
            shared = state.lock().unwrap();
        }

        let settings = shared.config.settings.clone();
        let paused = shared.paused;

        let foreground_key = foreground
            .as_ref()
            .and_then(|window| self.processes.get(&window.pid))
            .and_then(|info| info.key.clone())
            .filter(|key| shared.catalog.apps.contains_key(key));

        let fullscreen = foreground.as_ref().is_some_and(|window| window.fullscreen);
        let idle = probe::idle_seconds() >= settings.idle_minutes * 60
            && !(fullscreen && settings.fullscreen_counts_as_active);

        let active = if idle || paused {
            None
        } else {
            foreground_key.clone()
        };

        // Applications lancées : celles du catalogue dont un processus vit.
        let mut running: BTreeMap<String, i64> = BTreeMap::new();
        for info in self.processes.values() {
            if let Some(key) = &info.key
                && shared.catalog.apps.contains_key(key)
                && !probe::is_always_running(key)
            {
                let started = info.started_ms.unwrap_or(i64::MAX);
                running
                    .entry(key.clone())
                    .and_modify(|earliest| *earliest = (*earliest).min(started))
                    .or_insert(started);
            }
        }
        let open: HashSet<String> = running.keys().cloned().collect();

        let day = &mut shared.day;

        if whole > 0 && !paused {
            let start = second.saturating_sub(whole as DaySecond);

            if let Some(key) = &active {
                let app = day.apps.entry(key.clone()).or_default();
                app.active += whole;
                app.hours[hour] += whole as u32;
                extend_or_push(&mut day.focus, key, start, second);
            }

            for key in &open {
                day.apps.entry(key.clone()).or_default().open += whole;

                match self.open_sessions.get(key) {
                    Some(&index) if index < day.sessions.len() => {
                        day.sessions[index].end = second;
                    }
                    _ => {
                        day.sessions.push(Span {
                            app: key.clone(),
                            start,
                            end: second,
                        });
                        self.open_sessions
                            .insert(key.clone(), day.sessions.len() - 1);
                    }
                }
            }
        }
        self.open_sessions.retain(|key, _| open.contains(key));

        // Un lancement, c'est une application qui n'était pas ouverte au tour
        // précédent. Au premier tour, rien n'est un lancement : le programme
        // vient seulement d'ouvrir les yeux.
        if !self.first_tick {
            for key in open.difference(&self.previously_open) {
                day.apps.entry(key.clone()).or_default().launches += 1;
            }
        }
        self.first_tick = false;
        self.previously_open = open.clone();

        let foreground_since = active.as_ref().and_then(|key| {
            day.focus
                .last()
                .filter(|span| &span.app == key)
                .map(|span| span.start)
        });

        shared.live = Live {
            foreground: foreground_key,
            foreground_since,
            idle,
            paused,
            running: running
                .into_iter()
                .map(|(key, started)| (key, if started == i64::MAX { 0 } else { started }))
                .collect(),
        };

        let actions = self.evaluate_budgets(&shared, &open);
        drop(shared);

        for notice in actions {
            self.host.notify(notice);
        }
        self.run_pending_closes(&open);

        if self.last_save.elapsed() >= SAVE_EVERY {
            self.save();
            self.last_save = Instant::now();
        }
    }

    /// Passage à minuit : la journée écoulée est close et enregistrée.
    fn roll_over(&mut self, date: NaiveDate) {
        // La journée close est écrite avant d'être remplacée, verrou tenu :
        // l'historique ne peut pas lire entre-temps un fichier incomplet.
        let previous = {
            let mut shared = self.shared.lock().unwrap();
            for index in self.open_sessions.values() {
                if let Some(session) = shared.day.sessions.get_mut(*index) {
                    session.end = DAY_SECONDS;
                }
            }
            if let Err(error) = shared.day.save() {
                log_line!("journée {} non enregistrée : {error:#}", shared.day.date);
            }
            std::mem::replace(&mut shared.day, DayRecord::load(date))
        };

        self.open_sessions.clear();
        self.progress.clear();
        self.pending.clear();
        self.host.broadcast(
            "screen-time.day-changed",
            serde_json::json!({ "date": previous.date }),
        );
        log_line!("nouvelle journée : {date}");
    }

    // --- Budgets -----------------------------------------------------------

    fn evaluate_budgets(&mut self, shared: &Shared, open: &HashSet<String>) -> Vec<Notice> {
        let settings = &shared.config.settings;
        let mut notices = Vec::new();

        for budget in shared.config.budgets.iter().filter(|budget| budget.enabled) {
            let usage = shared.day.apps.get(&budget.app);
            let used = match budget.metric {
                Metric::Active => usage.map_or(0, |app| app.active),
                Metric::Open => usage.map_or(0, |app| app.open),
            };
            let extension = shared.day.extensions.get(&budget.app).copied().unwrap_or(0);
            let limit = u64::from(budget.minutes + extension) * 60;
            let running = open.contains(&budget.app);

            let progress = self.progress.entry(budget.app.clone()).or_default();
            if progress.limit != limit {
                // Limite modifiée (rallonge, réglage) : les avertissements
                // donnés pour l'ancienne ne valent plus.
                *progress = BudgetProgress {
                    limit,
                    ..Default::default()
                };
            }

            let info = shared.catalog.apps.get(&budget.app);
            let name = shared.catalog.name_of(&budget.app);
            let icon = info.and_then(|info| info.icon.clone());

            if used < limit {
                self.pending.remove(&budget.app);
                let remaining = limit - used;
                let warn = u64::from(settings.warn_minutes) * 60;
                if warn > 0 && remaining <= warn && !progress.warned_soon && running {
                    progress.warned_soon = true;
                    notices.push(Notice {
                        id: notice_id(&budget.app),
                        module: super::ID.to_string(),
                        title: "Budget bientôt atteint".to_string(),
                        body: format!(
                            "Il reste {} de {name} aujourd’hui.",
                            format_duration(remaining)
                        ),
                        icon,
                        tone: "warning",
                        countdown: None,
                        timeout: Some(15),
                        actions: vec![NoticeAction::new("dismiss", "Compris", true)],
                    });
                }
                continue;
            }

            let extend_label = format!("+\u{a0}{}\u{a0}min", settings.extension_minutes);

            match budget.action {
                Action::Notify => {
                    let every = u64::from(settings.reminder_minutes) * 60;
                    let level = (used - limit) / every.max(60);

                    if !progress.reached {
                        progress.reached = true;
                        progress.reminders = 0;
                        notices.push(Notice {
                            id: notice_id(&budget.app),
                            module: super::ID.to_string(),
                            title: "Budget atteint".to_string(),
                            body: format!(
                                "Vous avez utilisé tout le temps prévu pour {name} aujourd’hui ({}).",
                                format_duration(limit)
                            ),
                            icon,
                            tone: "critical",
                            countdown: None,
                            timeout: Some(30),
                            actions: vec![
                                NoticeAction::new("extend", &extend_label, false),
                                NoticeAction::new("dismiss", "Compris", true),
                            ],
                        });
                    } else if level > progress.reminders && running {
                        progress.reminders = level;
                        notices.push(Notice {
                            id: notice_id(&budget.app),
                            module: super::ID.to_string(),
                            title: "Budget dépassé".to_string(),
                            body: format!(
                                "{name} : {} au-delà du budget du jour.",
                                format_duration(used - limit)
                            ),
                            icon,
                            tone: "critical",
                            countdown: None,
                            timeout: Some(30),
                            actions: vec![NoticeAction::new("dismiss", "Compris", true)],
                        });
                    }
                }
                Action::Close => {
                    if running && !self.pending.contains_key(&budget.app) {
                        let grace = settings.close_grace_seconds;
                        self.pending.insert(
                            budget.app.clone(),
                            PendingClose {
                                deadline: Instant::now() + Duration::from_secs(u64::from(grace)),
                                close_sent: None,
                            },
                        );
                        log_line!("budget épuisé, fermeture de {name} dans {grace} s");
                        notices.push(Notice {
                            id: notice_id(&budget.app),
                            module: super::ID.to_string(),
                            title: "Budget épuisé".to_string(),
                            body: format!("{name} va être fermé. Enregistrez ce qui doit l’être."),
                            icon,
                            tone: "critical",
                            countdown: Some(grace),
                            timeout: None,
                            actions: vec![
                                NoticeAction::new("extend", &extend_label, false),
                                NoticeAction::new("close-now", "Fermer maintenant", true),
                            ],
                        });
                    }
                }
            }
        }

        notices
    }

    fn pids_of(&self, app: &str) -> HashSet<u32> {
        self.processes
            .iter()
            .filter(|(_, info)| info.key.as_deref() == Some(app))
            .map(|(pid, _)| *pid)
            .collect()
    }

    /// Exécute les fermetures dont le délai de grâce est écoulé : d'abord une
    /// demande polie aux fenêtres, puis, si l'application résiste, l'arrêt du
    /// processus.
    fn run_pending_closes(&mut self, open: &HashSet<String>) {
        let mut finished = Vec::new();
        let pids_by_app: HashMap<String, HashSet<u32>> = self
            .pending
            .keys()
            .map(|app| (app.clone(), self.pids_of(app)))
            .collect();

        for (app, pending) in &mut self.pending {
            if !open.contains(app) {
                if pending.close_sent.is_some() {
                    finished.push(app.clone());
                } else {
                    // Fermée par l'utilisateur avant l'échéance.
                    self.host.dismiss(&notice_id(app));
                    finished.push(app.clone());
                }
                continue;
            }

            let now = Instant::now();
            match pending.close_sent {
                None if now >= pending.deadline => {
                    let windows = probe::close_windows(&pids_by_app[app]);
                    log_line!("fermeture demandée à {app} ({windows} fenêtre(s))");
                    pending.close_sent = Some(now);
                }
                Some(sent) if sent.elapsed() >= FORCE_CLOSE_AFTER => {
                    for &pid in &pids_by_app[app] {
                        if probe::terminate(pid) {
                            log_line!("processus {pid} de {app} arrêté");
                        }
                    }
                    finished.push(app.clone());
                }
                _ => {}
            }
        }

        for app in finished {
            if let Some(pending) = self.pending.remove(&app)
                && pending.close_sent.is_some()
            {
                let (name, icon) = {
                    let shared = self.shared.lock().unwrap();
                    (
                        shared.catalog.name_of(&app),
                        shared
                            .catalog
                            .apps
                            .get(&app)
                            .and_then(|info| info.icon.clone()),
                    )
                };
                self.host.notify(Notice {
                    id: notice_id(&app),
                    module: super::ID.to_string(),
                    title: "Application fermée".to_string(),
                    body: format!("{name} a été fermé : le budget du jour est épuisé."),
                    icon,
                    tone: "info",
                    countdown: None,
                    timeout: Some(12),
                    actions: vec![NoticeAction::new("dismiss", "Compris", true)],
                });
            }
        }
    }
}

pub fn notice_id(app: &str) -> String {
    format!("budget:{app}")
}

fn extend_or_push(spans: &mut Vec<Span>, key: &str, start: DaySecond, end: DaySecond) {
    if let Some(last) = spans.last_mut()
        && last.app == key
        && end.saturating_sub(last.end) <= SPAN_TOLERANCE
    {
        last.end = end;
        return;
    }

    spans.push(Span {
        app: key.to_string(),
        start,
        end,
    });
}

/// « 1 h 05 », « 12 min », « 40 s », avec espaces insécables.
pub fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;

    if hours > 0 {
        format!("{hours}\u{a0}h\u{a0}{minutes:02}")
    } else if minutes > 0 {
        format!("{minutes}\u{a0}min")
    } else {
        format!("{seconds}\u{a0}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_merge_across_small_gaps() {
        let mut spans = Vec::new();
        extend_or_push(&mut spans, "a", 10, 11);
        extend_or_push(&mut spans, "a", 11, 14);
        extend_or_push(&mut spans, "b", 14, 15);
        extend_or_push(&mut spans, "a", 15, 16);
        extend_or_push(&mut spans, "a", 40, 41);
        assert_eq!(spans.len(), 4);
        assert_eq!(spans[0].end, 14);
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(format_duration(42), "42\u{a0}s");
        assert_eq!(format_duration(12 * 60 + 5), "12\u{a0}min");
        assert_eq!(format_duration(3600 + 5 * 60), "1\u{a0}h\u{a0}05");
    }
}
