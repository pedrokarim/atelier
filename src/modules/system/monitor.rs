//! Relevés successifs. Le processeur se mesure entre deux relevés, et un
//! processus n'est signalé pour lui qu'après un effort soutenu : il faut donc
//! se souvenir du relevé précédent.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::analysis::{CPU_PERCENT, CPU_SECONDS, Snapshot};
use super::probe;

/// Un relevé plus récent que cela est réutilisé : plusieurs widgets qui se
/// rafraîchissent ensemble ne relancent pas chacun le leur.
const FRESH: Duration = Duration::from_millis(1500);

/// Un processus, distingué de ses successeurs au même numéro.
type Identity = (u32, i64);

pub struct Monitor {
    /// Relevé fixe du mode démonstration (`demo.json`), pour l'audit de mise
    /// en page : rien de réel n'y est mesuré ni arrêté.
    demo: Option<Snapshot>,
    last: Option<(Instant, Snapshot)>,
    stale: bool,
    cpu_times: HashMap<Identity, u64>,
    busy_since: HashMap<Identity, Instant>,
    cores: f64,
}

impl Monitor {
    pub fn new(demo: Option<Snapshot>) -> Self {
        let demo = demo.map(|mut snapshot| {
            // Un processus qui apparaît occupé dans le relevé l'est depuis
            // assez longtemps pour être signalé.
            snapshot.busy = snapshot
                .processes
                .iter()
                .filter(|process| process.cpu >= CPU_PERCENT)
                .map(|process| (process.pid, CPU_SECONDS))
                .collect();
            snapshot
        });

        Self {
            demo,
            last: None,
            stale: false,
            cpu_times: HashMap::new(),
            busy_since: HashMap::new(),
            cores: std::thread::available_parallelism().map_or(1, |count| count.get()) as f64,
        }
    }

    pub fn is_demo(&self) -> bool {
        self.demo.is_some()
    }

    /// Le prochain relevé sera neuf : après une action, la page doit en voir
    /// l'effet.
    pub fn invalidate(&mut self) {
        self.stale = true;
    }

    pub fn snapshot(&mut self) -> Snapshot {
        if let Some(demo) = &self.demo {
            return demo.clone();
        }
        if let Some((at, snapshot)) = &self.last
            && !self.stale
            && at.elapsed() < FRESH
        {
            return snapshot.clone();
        }

        let now = Instant::now();
        let elapsed = self
            .last
            .as_ref()
            .map(|(at, _)| now.duration_since(*at).as_secs_f64());
        let mut processes = probe::processes();
        let mut cpu_times = HashMap::with_capacity(processes.len());
        let mut busy_since = HashMap::new();
        let mut busy = HashMap::new();

        for process in &mut processes {
            let identity = (process.pid, process.started.unwrap_or(0));
            if let (Some(elapsed), Some(previous)) = (elapsed, self.cpu_times.get(&identity))
                && elapsed > 0.0
            {
                let seconds = process.cpu_time.saturating_sub(*previous) as f64 / 10_000_000.0;
                process.cpu = (seconds / elapsed / self.cores * 100.0).clamp(0.0, 100.0);
            }
            cpu_times.insert(identity, process.cpu_time);

            if process.cpu >= CPU_PERCENT {
                let since = self.busy_since.get(&identity).copied().unwrap_or(now);
                busy_since.insert(identity, since);
                busy.insert(process.pid, now.duration_since(since).as_secs());
            }
        }

        self.cpu_times = cpu_times;
        self.busy_since = busy_since;
        let snapshot = Snapshot {
            memory: probe::memory(),
            processes,
            busy,
        };
        self.last = Some((now, snapshot.clone()));
        self.stale = false;
        snapshot
    }
}
