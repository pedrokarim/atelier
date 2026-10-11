//! Module « Temps d'écran » : le temps passé dans chaque application, et des
//! budgets quotidiens qui préviennent ou ferment l'application.
//!
//! Deux mesures, parce qu'elles répondent à deux questions :
//!
//! - **au premier plan** : l'application est devant l'utilisateur, et il est
//!   là (clavier ou souris actifs, ou plein écran) ;
//! - **ouvert** : un processus de l'application est lancé, qu'on le regarde
//!   ou non. C'est la réponse à « depuis quand le jeu tourne-t-il ? ».

mod config;
mod probe;
mod report;
mod store;
mod tracker;

use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::{Context, Result, anyhow};
use chrono::NaiveDate;
use serde_json::{Value, json};

use self::config::{Budget, Config, Settings};
use self::store::{Catalog, DayRecord, History};
use self::tracker::{Command, Live, Shared, SharedState, Tracker};
use super::{Module, ModuleContext};
use crate::ipc::Responder;
use crate::logging::log_line;

pub const ID: &str = "screen-time";

fn data_dir() -> Result<PathBuf> {
    crate::paths::module_dir(ID)
}

pub struct ScreenTime {
    shared: SharedState,
    history: Mutex<History>,
    commands: Mutex<Option<Sender<Command>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl ScreenTime {
    pub fn new() -> Self {
        let shared = Shared {
            day: DayRecord::load(report::today()),
            catalog: Catalog::load(),
            config: Config::load(),
            live: Live::default(),
            paused: false,
        };

        Self {
            shared: Arc::new(Mutex::new(shared)),
            history: Mutex::new(History::default()),
            commands: Mutex::new(None),
            thread: Mutex::new(None),
        }
    }

    fn command(&self, command: Command) {
        if let Some(sender) = self.commands.lock().unwrap().as_ref() {
            let _ = sender.send(command);
        }
    }

    fn save_config(&self, shared: &Shared) -> Result<()> {
        shared.config.save()
    }

    fn date_param(params: &Value) -> Result<NaiveDate> {
        match params.get("date").and_then(Value::as_str) {
            Some(text) => NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .with_context(|| format!("date invalide : {text}")),
            None => Ok(report::today()),
        }
    }

    fn key_param(params: &Value) -> Result<String> {
        params
            .get("key")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("application non précisée"))
    }

    fn dispatch(&self, method: &str, params: Value) -> Result<Value> {
        let mut shared = self.shared.lock().unwrap();

        match method {
            "live" => Ok(report::live(&shared)),

            "catalog" => Ok(report::catalog(&shared)),

            "day" => {
                let date = Self::date_param(&params)?;
                Ok(report::day(
                    &shared,
                    &mut self.history.lock().unwrap(),
                    date,
                ))
            }

            "range" => {
                let days = params
                    .get("days")
                    .and_then(Value::as_i64)
                    .unwrap_or(7)
                    .clamp(1, 366);
                let end = Self::date_param(&params)?;
                Ok(report::range(
                    &shared,
                    &mut self.history.lock().unwrap(),
                    end,
                    days,
                ))
            }

            "apps" => Ok(report::apps(&shared, &mut self.history.lock().unwrap())),

            "app" => {
                let key = Self::key_param(&params)?;
                Ok(report::app(
                    &shared,
                    &mut self.history.lock().unwrap(),
                    &key,
                ))
            }

            "budgets" => Ok(json!({
                "budgets": report::budget_statuses(&shared),
                "catalogVersion": shared.catalog.version,
            })),

            "budget.save" => {
                let budget: Budget =
                    serde_json::from_value(params.get("budget").cloned().unwrap_or(Value::Null))
                        .context("budget invalide")?;
                if budget.minutes == 0 {
                    return Err(anyhow!("un budget doit durer au moins une minute"));
                }
                if !shared.catalog.apps.contains_key(&budget.app) {
                    return Err(anyhow!("application inconnue"));
                }
                log_line!(
                    "budget enregistré : {} min pour {}",
                    budget.minutes,
                    budget.app
                );
                shared.config.upsert_budget(budget);
                self.save_config(&shared)?;
                Ok(json!(report::budget_statuses(&shared)))
            }

            "budget.delete" => {
                let key = Self::key_param(&params)?;
                shared.config.remove_budget(&key);
                self.save_config(&shared)?;
                Ok(json!(report::budget_statuses(&shared)))
            }

            "settings" => Ok(json!(shared.config.settings)),

            "settings.save" => {
                let settings: Settings =
                    serde_json::from_value(params.get("settings").cloned().unwrap_or(Value::Null))
                        .context("réglages invalides")?;
                shared.config.settings = settings.clamp();
                self.save_config(&shared)?;
                Ok(json!(shared.config.settings))
            }

            "pause" => {
                shared.paused = params
                    .get("paused")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                log_line!(
                    "suivi du temps d'écran {}",
                    if shared.paused { "suspendu" } else { "repris" }
                );
                Ok(json!({ "paused": shared.paused }))
            }

            "app.rename" => {
                let key = Self::key_param(&params)?;
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string);
                let info = shared
                    .catalog
                    .apps
                    .get_mut(&key)
                    .ok_or_else(|| anyhow!("application inconnue"))?;
                info.custom_name = name;
                shared.catalog.version += 1;
                shared.catalog.save()?;
                Ok(report::catalog(&shared))
            }

            "app.hide" => {
                let key = Self::key_param(&params)?;
                let hidden = params
                    .get("hidden")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let info = shared
                    .catalog
                    .apps
                    .get_mut(&key)
                    .ok_or_else(|| anyhow!("application inconnue"))?;
                info.hidden = hidden;
                shared.catalog.version += 1;
                shared.catalog.save()?;
                Ok(report::catalog(&shared))
            }

            "extend" => {
                let key = Self::key_param(&params)?;
                drop(shared);
                self.command(Command::Extend(key));
                Ok(Value::Null)
            }

            "open.data" => {
                let directory = data_dir()?;
                std::fs::create_dir_all(&directory)?;
                crate::shell::open(&directory)?;
                Ok(Value::Null)
            }

            other => Err(anyhow!("méthode inconnue : {other}")),
        }
    }
}

impl Module for ScreenTime {
    fn id(&self) -> &'static str {
        ID
    }

    fn script(&self) -> &'static str {
        include_str!("../../../ui/modules/screen-time.js")
    }

    fn default_widgets(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("screen-time.today", "m"),
            ("screen-time.now", "s"),
            ("screen-time.top", "l"),
            ("screen-time.running", "m"),
            ("screen-time.week", "m"),
            ("screen-time.budgets", "m"),
        ]
    }

    fn start(&self, context: ModuleContext) {
        let (sender, receiver) = channel();
        *self.commands.lock().unwrap() = Some(sender);

        let tracker = Tracker::new(self.shared.clone(), context.host);
        let handle = std::thread::Builder::new()
            .name("screen-time".into())
            .spawn(move || tracker.run(receiver));

        match handle {
            Ok(handle) => *self.thread.lock().unwrap() = Some(handle),
            Err(error) => log_line!("fil de suivi non démarré : {error}"),
        }
    }

    fn handle(&self, method: &str, params: Value, reply: Responder) {
        reply.result(self.dispatch(method, params));
    }

    fn notice_action(&self, notice: &str, action: &str) {
        let Some(app) = notice.strip_prefix("budget:") else {
            return;
        };

        match action {
            "extend" => self.command(Command::Extend(app.to_string())),
            "close-now" => self.command(Command::CloseNow(app.to_string())),
            _ => {}
        }
    }

    fn tray_status(&self) -> Option<String> {
        let shared = self.shared.lock().unwrap();
        let total = shared.day.total_active();
        let mut line = format!(
            "Temps d’écran aujourd’hui\u{a0}: {}",
            tracker::format_duration(total)
        );
        if shared.paused {
            line.push_str(" (suspendu)");
        }
        Some(line)
    }

    fn flush(&self) {
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

    fn stop(&self) {
        self.command(Command::Stop);
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}
