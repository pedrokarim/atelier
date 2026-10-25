//! Ce qui encombre l'ordinateur, à partir d'un relevé des processus : les
//! applications qui pèsent le plus, et les processus qui ont l'air anormaux.
//!
//! Tout ici est du calcul, sans appel à Windows : les seuils se testent sur
//! des relevés fabriqués.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

const GIB: u64 = 1024 * 1024 * 1024;

/// Un programme ordinaire garde quelques milliers d'objets du noyau ouverts
/// (fichiers, clés de registre, fils). Bien au-delà, il en ouvre sans jamais
/// les refermer, et chacun immobilise de la mémoire du noyau.
pub const HANDLES_WARNING: u32 = 50_000;
pub const HANDLES_CRITICAL: u32 = 500_000;

/// Une application est signalée au-delà de cette part de la mémoire physique,
/// et jamais sous `MEMORY_FLOOR`.
pub const MEMORY_SHARE: f64 = 0.12;
pub const MEMORY_CRITICAL_SHARE: f64 = 0.40;
const MEMORY_FLOOR: u64 = 2 * GIB;

/// Processeur, en part de la machine entière : un processus sans fenêtre qui
/// le sollicite ainsi pendant `CPU_SECONDS` travaille dans le dos de
/// l'utilisateur.
pub const CPU_PERCENT: f64 = 20.0;
pub const CPU_SECONDS: u64 = 30;

/// Âge à partir duquel un processus resté seul est signalé.
const ORPHAN_MINUTES: i64 = 10;

/// Le noyau occupe d'ordinaire 1 à 1,5 Go. Au-delà de cette part, et jamais
/// sous `KERNEL_FLOOR`, quelque chose le gonfle.
const KERNEL_SHARE: f64 = 0.12;
const KERNEL_CRITICAL_SHARE: f64 = 0.25;
const KERNEL_FLOOR: u64 = 2 * GIB;

/// Mémoire disponible sous laquelle Windows commence à écrire sur le disque.
const PRESSURE_WARNING_SHARE: f64 = 0.15;
const PRESSURE_CRITICAL_SHARE: f64 = 0.08;

/// Mémoire engagée, rapportée à ce que la mémoire et le fichier d'échange
/// peuvent fournir.
const COMMIT_SHARE: f64 = 0.90;

/// Processus dont l'arrêt ferait tomber la session ou Windows lui-même.
const CRITICAL_NAMES: &[&str] = &[
    "system",
    "registry",
    "memory compression",
    "secure system",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "svchost.exe",
    "fontdrvhost.exe",
    "dwm.exe",
    "sihost.exe",
    "ctfmon.exe",
];

/// État de la mémoire de l'ordinateur, en octets.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Memory {
    pub total: u64,
    pub available: u64,
    pub commit: u64,
    pub commit_limit: u64,
    pub kernel_paged: u64,
    pub kernel_nonpaged: u64,
    pub cache: u64,
    pub handles: u32,
    pub processes: u32,
    pub threads: u32,
}

impl Memory {
    pub fn kernel(&self) -> u64 {
        self.kernel_paged + self.kernel_nonpaged
    }

    fn share(&self, bytes: u64) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            bytes as f64 / self.total as f64
        }
    }
}

/// Un processus, tel que relevé.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Process {
    pub pid: u32,
    pub parent: u32,
    /// Nom de l'exécutable : `find.exe`.
    pub name: String,
    pub path: Option<String>,
    /// Lancement, en millisecondes depuis l'époque Unix.
    pub started: Option<i64>,
    /// Mémoire physique que le processus est seul à occuper.
    pub private_bytes: u64,
    pub commit: u64,
    pub handles: u32,
    pub threads: u32,
    /// Session Windows : 0 pour les services.
    pub session: Option<u32>,
    /// Possède au moins une fenêtre visible.
    pub windowed: bool,
    /// Programme en ligne de commande : sans terminal, personne ne lit ce
    /// qu'il affiche.
    pub console: bool,
    /// Part de la machine entière, en pour cent, depuis le relevé précédent.
    pub cpu: f64,
    /// Temps de processeur cumulé, en centaines de nanosecondes.
    #[serde(skip)]
    pub cpu_time: u64,
}

impl Process {
    /// Clé de l'application : chemin de l'exécutable en minuscules, comme
    /// dans le catalogue du module Temps d'écran ; à défaut, son nom.
    pub fn key(&self) -> String {
        self.path.as_deref().unwrap_or(&self.name).to_lowercase()
    }

    fn age_minutes(&self, now_ms: i64) -> i64 {
        self.started
            .map(|started| (now_ms - started) / 60_000)
            .unwrap_or(0)
    }
}

/// Un relevé complet.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Snapshot {
    pub memory: Memory,
    pub processes: Vec<Process>,
    /// Depuis combien de secondes chaque processus sollicite le processeur
    /// au-delà de `CPU_PERCENT`.
    #[serde(skip)]
    pub busy: HashMap<u32, u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Info,
    Warning,
    Critical,
}

/// Une raison de signaler un processus ou une application.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reason {
    /// `handles`, `memory`, `cpu` ou `orphan`.
    pub kind: &'static str,
    pub tone: Tone,
    pub value: f64,
    /// Part de la mémoire physique, pour `memory`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share: Option<f64>,
}

/// Processus visé par une action. L'heure de lancement garantit qu'on arrête
/// bien celui qui a été affiché, et pas un successeur qui aurait hérité de
/// son numéro.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub pid: u32,
    pub started: Option<i64>,
}

/// Ce qui mérite l'attention : un processus, ou une application entière
/// quand c'est leur somme qui pèse.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suspect {
    pub id: String,
    pub key: String,
    pub name: String,
    pub path: Option<String>,
    pub targets: Vec<Target>,
    pub started: Option<i64>,
    pub private_bytes: u64,
    pub handles: u64,
    pub cpu: f64,
    pub windowed: bool,
    pub protected: bool,
    pub tone: Tone,
    pub reasons: Vec<Reason>,
}

impl Suspect {
    /// Mérite une notification : une fuite avérée, ou une application sans
    /// fenêtre qui prend presque toute la mémoire. Une application ouverte
    /// devant l'utilisateur, si gourmande soit-elle, est son choix.
    pub fn deserves_notice(&self) -> bool {
        !self.protected
            && self.reasons.iter().any(|reason| {
                reason.tone == Tone::Critical
                    && (reason.kind == "handles" || (reason.kind == "memory" && !self.windowed))
            })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub pid: u32,
    pub started: Option<i64>,
    pub private_bytes: u64,
    pub handles: u32,
    pub threads: u32,
    pub cpu: f64,
    pub windowed: bool,
    pub protected: bool,
    pub orphan: bool,
}

/// Les processus d'une même application, additionnés.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub key: String,
    pub name: String,
    pub path: Option<String>,
    pub count: usize,
    pub private_bytes: u64,
    pub commit: u64,
    pub handles: u64,
    pub cpu: f64,
    pub windowed: bool,
    /// Aucun de ses processus ne peut être arrêté.
    pub protected: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub processes: Vec<Entry>,
}

/// Où passe la mémoire physique.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Breakdown {
    pub used: u64,
    /// Mémoire que chaque processus est seul à occuper, additionnée.
    pub apps: u64,
    pub kernel: u64,
    /// Bibliothèques partagées, fichiers projetés, pilotes : le reste.
    pub other: u64,
    pub available: u64,
}

/// Alerte sur l'ordinateur entier.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    /// `kernel`, `pressure` ou `commit`.
    pub kind: &'static str,
    pub tone: Tone,
    pub value: u64,
    pub share: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub memory: Memory,
    pub breakdown: Breakdown,
    pub alerts: Vec<Alert>,
    pub suspects: Vec<Suspect>,
    pub groups: Vec<Group>,
}

/// Ce qui entoure l'analyse : l'heure, Atelier lui-même, et les applications
/// que l'utilisateur ne veut plus voir signalées.
pub struct Context<'a> {
    pub now_ms: i64,
    pub own_pid: u32,
    pub ignored: &'a HashSet<String>,
}

pub fn analyze(snapshot: &Snapshot, context: &Context) -> Report {
    let processes = &snapshot.processes;
    let memory = &snapshot.memory;
    let own = descendants(processes, context.own_pid);
    let protected: HashSet<u32> = processes
        .iter()
        .filter(|process| is_protected(process, &own))
        .map(|process| process.pid)
        .collect();
    let orphans: HashSet<u32> = processes
        .iter()
        .filter(|process| is_orphan(process, processes, context.now_ms))
        .map(|process| process.pid)
        .collect();

    let groups = group(processes, &protected, &orphans);
    let suspects = suspects(snapshot, &groups, &protected, &orphans, context.ignored);

    let apps: u64 = processes.iter().map(|process| process.private_bytes).sum();
    let used = memory.total.saturating_sub(memory.available);
    let kernel = memory.kernel();
    let breakdown = Breakdown {
        used,
        apps,
        kernel,
        other: used.saturating_sub(apps + kernel),
        available: memory.available,
    };

    Report {
        memory: memory.clone(),
        breakdown,
        alerts: alerts(memory),
        suspects,
        groups,
    }
}

/// Atelier et les processus qu'il a lancés (son moteur de rendu) : les
/// arrêter couperait l'interface sous les pieds de l'utilisateur.
fn descendants(processes: &[Process], root: u32) -> HashSet<u32> {
    let started: HashMap<u32, Option<i64>> = processes
        .iter()
        .map(|process| (process.pid, process.started))
        .collect();
    let mut found = HashSet::from([root]);
    loop {
        let before = found.len();
        for process in processes {
            if found.contains(&process.parent)
                && process.pid != process.parent
                && launched_by(process, started.get(&process.parent).copied().flatten())
            {
                found.insert(process.pid);
            }
        }
        if found.len() == before {
            return found;
        }
    }
}

/// Un parent ne peut pas être plus jeune que son enfant : sinon, son numéro
/// a été réattribué après la fin du vrai parent.
fn launched_by(child: &Process, parent_started: Option<i64>) -> bool {
    match (child.started, parent_started) {
        (Some(child), Some(parent)) => parent <= child,
        _ => true,
    }
}

fn is_protected(process: &Process, own: &HashSet<u32>) -> bool {
    process.pid <= 4
        || own.contains(&process.pid)
        || process.session.is_none_or(|session| session == 0)
        || CRITICAL_NAMES.contains(&process.name.to_lowercase().as_str())
}

fn in_windows_directory(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower
        .get(1..)
        .is_some_and(|rest| rest.starts_with(r":\windows\"))
}

/// Resté seul : une commande dont le terminal ou le script qui l'avait
/// lancée s'est fermé, sans fenêtre, et qui tourne depuis un moment. Les
/// applications de fond (synchronisation, pilotes) ne sont pas visées : elles
/// survivent normalement à leur lanceur, mais ce ne sont pas des commandes.
fn is_orphan(process: &Process, processes: &[Process], now_ms: i64) -> bool {
    if !process.console
        || process.windowed
        || process.session.is_none_or(|session| session == 0)
        || process.path.as_deref().is_none_or(in_windows_directory)
        || process.age_minutes(now_ms) < ORPHAN_MINUTES
    {
        return false;
    }

    !processes.iter().any(|candidate| {
        candidate.pid == process.parent
            && candidate.pid != process.pid
            && launched_by(process, candidate.started)
    })
}

fn group(processes: &[Process], protected: &HashSet<u32>, orphans: &HashSet<u32>) -> Vec<Group> {
    let mut groups: HashMap<String, Group> = HashMap::new();

    for process in processes.iter().filter(|process| process.pid != 0) {
        let group = groups.entry(process.key()).or_insert_with(|| Group {
            key: process.key(),
            name: process.name.clone(),
            path: process.path.clone(),
            count: 0,
            private_bytes: 0,
            commit: 0,
            handles: 0,
            cpu: 0.0,
            windowed: false,
            protected: true,
            processes: Vec::new(),
        });
        let is_protected = protected.contains(&process.pid);
        group.count += 1;
        group.private_bytes += process.private_bytes;
        group.commit += process.commit;
        group.handles += u64::from(process.handles);
        group.cpu += process.cpu;
        group.windowed |= process.windowed;
        group.protected &= is_protected;
        group.processes.push(Entry {
            pid: process.pid,
            started: process.started,
            private_bytes: process.private_bytes,
            handles: process.handles,
            threads: process.threads,
            cpu: process.cpu,
            windowed: process.windowed,
            protected: is_protected,
            orphan: orphans.contains(&process.pid),
        });
    }

    let mut groups: Vec<Group> = groups.into_values().collect();
    for group in &mut groups {
        group
            .processes
            .sort_by_key(|entry| std::cmp::Reverse(entry.private_bytes));
    }
    groups.sort_by(|a, b| {
        b.private_bytes
            .cmp(&a.private_bytes)
            .then_with(|| a.key.cmp(&b.key))
    });
    groups
}

fn suspects(
    snapshot: &Snapshot,
    groups: &[Group],
    protected: &HashSet<u32>,
    orphans: &HashSet<u32>,
    ignored: &HashSet<String>,
) -> Vec<Suspect> {
    let memory = &snapshot.memory;
    let mut found: Vec<Suspect> = Vec::new();

    // Raisons propres à un processus.
    for process in &snapshot.processes {
        if ignored.contains(&process.key()) {
            continue;
        }

        let mut reasons = Vec::new();
        if process.handles >= HANDLES_WARNING {
            reasons.push(Reason {
                kind: "handles",
                tone: if process.handles >= HANDLES_CRITICAL {
                    Tone::Critical
                } else {
                    Tone::Warning
                },
                value: f64::from(process.handles),
                share: None,
            });
        }
        let busy = snapshot.busy.get(&process.pid).copied().unwrap_or(0);
        if !process.windowed && process.cpu >= CPU_PERCENT && busy >= CPU_SECONDS {
            reasons.push(Reason {
                kind: "cpu",
                tone: Tone::Warning,
                value: process.cpu,
                share: None,
            });
        }
        if orphans.contains(&process.pid) {
            reasons.push(Reason {
                kind: "orphan",
                tone: Tone::Info,
                value: 0.0,
                share: None,
            });
        }

        if !reasons.is_empty() {
            found.push(Suspect {
                id: format!("pid:{}:{}", process.pid, process.started.unwrap_or(0)),
                key: process.key(),
                name: process.name.clone(),
                path: process.path.clone(),
                targets: vec![Target {
                    pid: process.pid,
                    started: process.started,
                }],
                started: process.started,
                private_bytes: process.private_bytes,
                handles: u64::from(process.handles),
                cpu: process.cpu,
                windowed: process.windowed,
                protected: protected.contains(&process.pid),
                tone: Tone::Info,
                reasons,
            });
        }
    }

    // Mémoire : c'est l'application entière qui pèse, tous processus
    // confondus (un navigateur en compte des dizaines).
    let threshold = ((memory.total as f64 * MEMORY_SHARE) as u64).max(MEMORY_FLOOR);
    for group in groups {
        if group.private_bytes < threshold || ignored.contains(&group.key) {
            continue;
        }
        let share = memory.share(group.private_bytes);
        let reason = Reason {
            kind: "memory",
            tone: if share >= MEMORY_CRITICAL_SHARE {
                Tone::Critical
            } else {
                Tone::Warning
            },
            value: group.private_bytes as f64,
            share: Some(share),
        };

        // Une application d'un seul processus déjà signalé : une seule
        // carte, avec toutes ses raisons.
        if group.count == 1
            && let Some(existing) = found.iter_mut().find(|suspect| suspect.key == group.key)
        {
            existing.reasons.insert(0, reason);
            continue;
        }

        let targets: Vec<Target> = group
            .processes
            .iter()
            .filter(|entry| !entry.protected)
            .map(|entry| Target {
                pid: entry.pid,
                started: entry.started,
            })
            .collect();
        let single = (group.count == 1).then(|| &group.processes[0]);
        found.push(Suspect {
            id: match single {
                Some(entry) => format!("pid:{}:{}", entry.pid, entry.started.unwrap_or(0)),
                None => format!("app:{}", group.key),
            },
            key: group.key.clone(),
            name: group.name.clone(),
            path: group.path.clone(),
            targets,
            started: single.and_then(|entry| entry.started),
            private_bytes: group.private_bytes,
            handles: group.handles,
            cpu: group.cpu,
            windowed: group.windowed,
            protected: group.protected,
            tone: Tone::Info,
            reasons: vec![reason],
        });
    }

    for suspect in &mut found {
        suspect.tone = suspect
            .reasons
            .iter()
            .map(|reason| reason.tone)
            .max()
            .unwrap_or(Tone::Info);
    }
    found.sort_by(|a, b| {
        b.tone
            .cmp(&a.tone)
            .then_with(|| b.private_bytes.cmp(&a.private_bytes))
    });
    found
}

fn alerts(memory: &Memory) -> Vec<Alert> {
    let mut alerts = Vec::new();
    if memory.total == 0 {
        return alerts;
    }

    let kernel = memory.kernel();
    let kernel_share = memory.share(kernel);
    if kernel_share >= KERNEL_SHARE && kernel >= KERNEL_FLOOR {
        alerts.push(Alert {
            kind: "kernel",
            tone: if kernel_share >= KERNEL_CRITICAL_SHARE {
                Tone::Critical
            } else {
                Tone::Warning
            },
            value: kernel,
            share: kernel_share,
        });
    }

    let available_share = memory.share(memory.available);
    if available_share < PRESSURE_WARNING_SHARE {
        alerts.push(Alert {
            kind: "pressure",
            tone: if available_share < PRESSURE_CRITICAL_SHARE {
                Tone::Critical
            } else {
                Tone::Warning
            },
            value: memory.available,
            share: available_share,
        });
    }

    if memory.commit_limit > 0 {
        let commit_share = memory.commit as f64 / memory.commit_limit as f64;
        if commit_share >= COMMIT_SHARE {
            alerts.push(Alert {
                kind: "commit",
                tone: Tone::Warning,
                value: memory.commit,
                share: commit_share,
            });
        }
    }

    alerts
}

/// « 7,3 Go », « 512 Mo ».
pub fn format_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["octets", "Ko", "Mo", "Go", "To"];
    let mut amount = value as f64;
    let mut unit = 0;
    while amount >= 1024.0 && unit < UNITS.len() - 1 {
        amount /= 1024.0;
        unit += 1;
    }
    let text = if unit >= 3 && amount < 100.0 {
        format!("{amount:.1}")
    } else {
        format!("{amount:.0}")
    };
    format!("{}\u{a0}{}", text.replace('.', ","), UNITS[unit])
}

/// « 7,3 millions », « 52 000 ».
pub fn format_count(value: u64) -> String {
    if value >= 1_000_000 {
        let millions = value as f64 / 1_000_000.0;
        let unit = if millions < 2.0 {
            "million"
        } else {
            "millions"
        };
        return format!("{}\u{a0}{unit}", format!("{millions:.1}").replace('.', ","));
    }

    let digits = value.to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push('\u{a0}');
        }
        text.push(digit);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;
    const HOUR: i64 = 3_600_000;

    fn memory() -> Memory {
        Memory {
            total: 32 * GIB,
            available: 12 * GIB,
            commit: 20 * GIB,
            commit_limit: 60 * GIB,
            kernel_paged: GIB / 2,
            kernel_nonpaged: GIB / 4,
            ..Default::default()
        }
    }

    fn process(pid: u32, parent: u32, name: &str) -> Process {
        Process {
            pid,
            parent,
            name: name.to_string(),
            path: Some(format!(r"C:\Programmes\{name}")),
            started: Some(NOW - 2 * HOUR),
            private_bytes: 100 * 1024 * 1024,
            handles: 800,
            threads: 12,
            session: Some(1),
            windowed: true,
            console: false,
            ..Default::default()
        }
    }

    fn run(processes: Vec<Process>, ignored: &[&str]) -> Report {
        let ignored: HashSet<String> = ignored.iter().map(|key| key.to_string()).collect();
        let snapshot = Snapshot {
            memory: memory(),
            processes,
            busy: HashMap::new(),
        };
        analyze(
            &snapshot,
            &Context {
                now_ms: NOW,
                own_pid: 900,
                ignored: &ignored,
            },
        )
    }

    fn launcher() -> Process {
        Process {
            started: Some(NOW - 3 * HOUR),
            ..process(10, 4, "explorer.exe")
        }
    }

    #[test]
    fn ordinary_processes_are_not_suspects() {
        let report = run(vec![launcher(), process(20, 10, "editor.exe")], &[]);
        assert!(report.suspects.is_empty());
        assert!(report.alerts.is_empty());
    }

    #[test]
    fn handle_leak_is_critical_and_worth_a_notice() {
        let mut leak = process(30, 10, "find.exe");
        leak.handles = 7_250_506;
        leak.windowed = false;
        let report = run(vec![launcher(), leak], &[]);

        let suspect = &report.suspects[0];
        assert_eq!(suspect.tone, Tone::Critical);
        assert_eq!(suspect.reasons[0].kind, "handles");
        assert_eq!(suspect.id, format!("pid:30:{}", NOW - 2 * HOUR));
        assert!(suspect.deserves_notice());
    }

    #[test]
    fn memory_is_measured_per_application() {
        // Dix processus de 400 Mo : aucun ne pèse seul, l'application si.
        let mut processes = vec![launcher()];
        for index in 0..10 {
            processes.push(Process {
                private_bytes: 400 * 1024 * 1024,
                ..process(100 + index, 10, "browser.exe")
            });
        }
        let report = run(processes, &[]);

        let suspect = &report.suspects[0];
        assert_eq!(suspect.reasons[0].kind, "memory");
        assert_eq!(suspect.targets.len(), 10);
        assert!(suspect.id.starts_with("app:"));
        // Ouverte devant l'utilisateur : pas de notification.
        assert!(!suspect.deserves_notice());
    }

    #[test]
    fn single_process_application_gets_one_card() {
        let mut game = process(40, 10, "game.exe");
        game.private_bytes = 14 * GIB;
        game.handles = 60_000;
        let report = run(vec![launcher(), game], &[]);

        assert_eq!(report.suspects.len(), 1);
        let kinds: Vec<_> = report.suspects[0]
            .reasons
            .iter()
            .map(|reason| reason.kind)
            .collect();
        assert_eq!(kinds, ["memory", "handles"]);
        assert_eq!(report.suspects[0].tone, Tone::Critical);
    }

    #[test]
    fn ignored_applications_are_not_reported() {
        let mut game = process(40, 10, "game.exe");
        game.private_bytes = 14 * GIB;
        let key = game.key();
        let report = run(vec![launcher(), game], &[&key]);
        assert!(report.suspects.is_empty());
        // Elle reste dans la liste des processus.
        assert!(report.groups.iter().any(|group| group.key == key));
    }

    #[test]
    fn orphan_needs_a_dead_parent_no_window_and_some_age() {
        let mut lone = process(50, 77, "node.exe");
        lone.windowed = false;
        lone.console = true;
        let report = run(vec![launcher(), lone.clone()], &[]);
        assert_eq!(report.suspects[0].reasons[0].kind, "orphan");
        assert_eq!(report.suspects[0].tone, Tone::Info);

        // Parent toujours là.
        let mut child = lone.clone();
        child.parent = 10;
        assert!(run(vec![launcher(), child], &[]).suspects.is_empty());

        // Trop jeune.
        let mut young = lone.clone();
        young.started = Some(NOW - 60_000);
        assert!(run(vec![launcher(), young], &[]).suspects.is_empty());

        // Composant de Windows.
        let mut system = lone.clone();
        system.path = Some(r"C:\Windows\System32\conhost.exe".into());
        assert!(run(vec![launcher(), system], &[]).suspects.is_empty());

        // Application de fond, pas une commande.
        let mut background = lone.clone();
        background.console = false;
        assert!(run(vec![launcher(), background], &[]).suspects.is_empty());
    }

    #[test]
    fn reused_parent_number_still_means_orphan() {
        // Le numéro 10 appartient désormais à un processus plus jeune.
        let mut lone = process(50, 10, "node.exe");
        lone.windowed = false;
        lone.console = true;
        let mut successor = launcher();
        successor.started = Some(NOW - HOUR);
        let report = run(vec![successor, lone], &[]);
        assert_eq!(report.suspects[0].reasons[0].kind, "orphan");
    }

    #[test]
    fn critical_and_own_processes_are_protected() {
        let mut service = process(60, 10, "svchost.exe");
        service.handles = 80_000;
        let mut webview = process(901, 900, "msedgewebview2.exe");
        webview.handles = 80_000;
        let atelier = process(900, 10, "atelier.exe");
        let mut session_zero = process(61, 10, "vendor-service.exe");
        session_zero.session = Some(0);
        session_zero.handles = 80_000;

        let report = run(
            vec![launcher(), atelier, service, webview, session_zero],
            &[],
        );
        assert_eq!(report.suspects.len(), 3);
        assert!(report.suspects.iter().all(|suspect| suspect.protected));
        assert!(!report.suspects.iter().any(Suspect::deserves_notice));
    }

    #[test]
    fn swollen_kernel_and_low_memory_raise_alerts() {
        let snapshot = Snapshot {
            memory: Memory {
                available: 2 * GIB,
                kernel_paged: 4 * GIB,
                kernel_nonpaged: 3 * GIB / 2,
                ..memory()
            },
            ..Default::default()
        };
        let report = analyze(
            &snapshot,
            &Context {
                now_ms: NOW,
                own_pid: 1,
                ignored: &HashSet::new(),
            },
        );
        let kinds: Vec<_> = report.alerts.iter().map(|alert| alert.kind).collect();
        assert_eq!(kinds, ["kernel", "pressure"]);
        assert_eq!(report.alerts[1].tone, Tone::Critical);
        assert_eq!(report.breakdown.used, 30 * GIB);
    }

    #[test]
    fn busy_background_process_is_reported_after_a_while() {
        let mut worker = process(70, 10, "indexer.exe");
        worker.windowed = false;
        worker.cpu = 45.0;
        let mut snapshot = Snapshot {
            memory: memory(),
            processes: vec![launcher(), worker],
            busy: HashMap::from([(70, 5)]),
        };
        let context = Context {
            now_ms: NOW,
            own_pid: 1,
            ignored: &HashSet::new(),
        };
        assert!(analyze(&snapshot, &context).suspects.is_empty());

        snapshot.busy.insert(70, CPU_SECONDS);
        let report = analyze(&snapshot, &context);
        assert_eq!(report.suspects[0].reasons[0].kind, "cpu");
    }

    #[test]
    fn formatting() {
        assert_eq!(format_bytes(7_838_000_000), "7,3\u{a0}Go");
        assert_eq!(format_bytes(512 * 1024 * 1024), "512\u{a0}Mo");
        assert_eq!(format_count(7_250_506), "7,3\u{a0}millions");
        assert_eq!(format_count(1_200_000), "1,2\u{a0}million");
        assert_eq!(format_count(52_000), "52\u{a0}000");
        assert_eq!(format_count(812), "812");
    }
}
