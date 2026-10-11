//! Les réponses aux pages et aux widgets : des vues calculées sur les données.

use std::collections::BTreeMap;

use chrono::{Duration, Local, NaiveDate};
use serde::Serialize;
use serde_json::{Value, json};

use super::config::{Budget, Metric};
use super::store::{DayRecord, DaySummary, History, Totals};
use super::tracker::Shared;

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn date_string(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Résumé d'un jour quelconque : l'état vivant pour aujourd'hui, le disque
/// pour les autres.
fn summary(shared: &Shared, history: &mut History, date: NaiveDate) -> DaySummary {
    if shared.day.date == date_string(date) {
        shared.day.summary()
    } else {
        history.past(date).clone()
    }
}

/// Moyenne des jours renseignés parmi les `days` jours précédant `date`.
fn average_before(
    shared: &Shared,
    history: &mut History,
    date: NaiveDate,
    days: i64,
) -> Option<u64> {
    let totals: Vec<u64> = (1..=days)
        .map(|offset| summary(shared, history, date - Duration::days(offset)).total_active)
        .filter(|total| *total > 0)
        .collect();

    (!totals.is_empty()).then(|| totals.iter().sum::<u64>() / totals.len() as u64)
}

#[derive(Serialize)]
pub struct BudgetStatus {
    #[serde(flatten)]
    pub budget: Budget,
    pub used: u64,
    pub limit: u64,
    pub extension: u32,
    /// `ok`, `soon` ou `over`.
    pub state: &'static str,
}

pub fn budget_statuses(shared: &Shared) -> Vec<BudgetStatus> {
    let warn = u64::from(shared.config.settings.warn_minutes) * 60;

    shared
        .config
        .budgets
        .iter()
        .map(|budget| {
            let usage = shared.day.apps.get(&budget.app);
            let used = match budget.metric {
                Metric::Active => usage.map_or(0, |app| app.active),
                Metric::Open => usage.map_or(0, |app| app.open),
            };
            let extension = shared.day.extensions.get(&budget.app).copied().unwrap_or(0);
            let limit = u64::from(budget.minutes + extension) * 60;
            let state = if used >= limit {
                "over"
            } else if limit - used <= warn.max(limit / 5) {
                "soon"
            } else {
                "ok"
            };

            BudgetStatus {
                budget: budget.clone(),
                used,
                limit,
                extension,
                state,
            }
        })
        .collect()
}

pub fn live(shared: &Shared) -> Value {
    let foreground_active = shared
        .live
        .foreground
        .as_ref()
        .and_then(|key| shared.day.apps.get(key))
        .map_or(0, |app| app.active);

    json!({
        "foreground": shared.live.foreground,
        "foregroundSince": shared.live.foreground_since,
        "foregroundToday": foreground_active,
        "idle": shared.live.idle,
        "paused": shared.paused,
        "running": shared.live.running,
        "total": shared.day.total_active(),
        "catalogVersion": shared.catalog.version,
    })
}

/// Détail d'une journée.
pub fn day(shared: &Shared, history: &mut History, date: NaiveDate) -> Value {
    let is_today = shared.day.date == date_string(date);
    let record = if is_today {
        shared.day.clone()
    } else {
        DayRecord::load(date)
    };

    let mut apps: Vec<Value> = record
        .apps
        .iter()
        .filter(|(key, _)| !shared.catalog.is_hidden(key))
        .map(|(key, usage)| {
            json!({
                "key": key,
                "active": usage.active,
                "open": usage.open,
                "launches": usage.launches,
                "hours": usage.hours,
            })
        })
        .collect();
    apps.sort_by_key(|app| {
        std::cmp::Reverse((
            app["active"].as_u64().unwrap_or(0),
            app["open"].as_u64().unwrap_or(0),
        ))
    });

    let mut hours = [0u64; 24];
    for (key, usage) in &record.apps {
        if shared.catalog.is_hidden(key) {
            continue;
        }
        for (hour, seconds) in usage.hours.iter().enumerate() {
            hours[hour] += u64::from(*seconds);
        }
    }

    let visible_spans = |spans: &[super::store::Span]| -> Vec<Value> {
        spans
            .iter()
            .filter(|span| !shared.catalog.is_hidden(&span.app))
            .map(|span| json!({ "app": span.app, "start": span.start, "end": span.end }))
            .collect()
    };

    let total: u64 = hours.iter().sum();

    json!({
        "date": record.date,
        "isToday": is_today,
        "total": total,
        "launches": record.apps.values().map(|app| app.launches).sum::<u32>(),
        "apps": apps,
        "hours": hours,
        "focus": visible_spans(&record.focus),
        "sessions": visible_spans(&record.sessions),
        "average7": average_before(shared, history, date, 7),
        "previous": summary(shared, history, date - Duration::days(1)).total_active,
        "budgets": if is_today { serde_json::to_value(budget_statuses(shared)).unwrap_or_default() } else { Value::Null },
        "live": if is_today { live(shared) } else { Value::Null },
        "catalogVersion": shared.catalog.version,
    })
}

/// Période de `days` jours se terminant à `end`.
pub fn range(shared: &Shared, history: &mut History, end: NaiveDate, days: i64) -> Value {
    let mut per_day = Vec::new();
    let mut apps: BTreeMap<String, (Totals, u32)> = BTreeMap::new();

    for offset in (0..days).rev() {
        let date = end - Duration::days(offset);
        let summary = summary(shared, history, date);
        let mut total = 0;

        for (key, totals) in &summary.apps {
            if shared.catalog.is_hidden(key) {
                continue;
            }
            total += totals.active;
            let entry = apps.entry(key.clone()).or_default();
            entry.0.add(totals);
            if totals.active > 0 || totals.open > 0 {
                entry.1 += 1;
            }
        }

        per_day.push(json!({ "date": date_string(date), "total": total }));
    }

    let mut ranking: Vec<Value> = apps
        .into_iter()
        .map(|(key, (totals, days_used))| {
            json!({
                "key": key,
                "active": totals.active,
                "open": totals.open,
                "launches": totals.launches,
                "days": days_used,
            })
        })
        .collect();
    ranking.sort_by_key(|app| std::cmp::Reverse(app["active"].as_u64().unwrap_or(0)));

    let totals: Vec<u64> = per_day
        .iter()
        .map(|day| day["total"].as_u64().unwrap_or(0))
        .collect();
    let recorded = totals.iter().filter(|total| **total > 0).count() as u64;
    let sum: u64 = totals.iter().sum();

    // La période précédente, de même durée, pour la comparaison.
    let previous: u64 = (days..days * 2)
        .map(|offset| {
            summary(shared, history, end - Duration::days(offset))
                .apps
                .iter()
                .filter(|(key, _)| !shared.catalog.is_hidden(key))
                .map(|(_, totals)| totals.active)
                .sum::<u64>()
        })
        .sum();

    json!({
        "end": date_string(end),
        "days": per_day,
        "apps": ranking,
        "total": sum,
        "average": sum.checked_div(recorded).unwrap_or(0),
        "recordedDays": recorded,
        "previousTotal": previous,
        "catalogVersion": shared.catalog.version,
    })
}

/// Toutes les applications connues, avec leurs totaux.
pub fn apps(shared: &Shared, history: &mut History) -> Value {
    let today = today();
    let week_start = today - Duration::days(6);
    let mut stats: BTreeMap<String, Value> = BTreeMap::new();

    let mut dates = History::recorded_dates();
    if !dates.contains(&today) {
        dates.push(today);
    }

    for date in dates {
        let summary = summary(shared, history, date);
        for (key, totals) in &summary.apps {
            let entry = stats.entry(key.clone()).or_insert_with(|| {
                json!({ "key": key, "today": 0, "week": 0, "total": 0, "open": 0, "launches": 0, "days": 0, "lastUsed": null })
            });
            let add = |entry: &mut Value, field: &str, value: u64| {
                entry[field] = json!(entry[field].as_u64().unwrap_or(0) + value);
            };
            add(entry, "total", totals.active);
            add(entry, "open", totals.open);
            add(entry, "launches", u64::from(totals.launches));
            if date == today {
                add(entry, "today", totals.active);
            }
            if date >= week_start {
                add(entry, "week", totals.active);
            }
            if totals.active > 0 || totals.open > 0 {
                add(entry, "days", 1);
                entry["lastUsed"] = json!(date_string(date));
            }
        }
    }

    // Les applications vues mais jamais comptées figurent aussi.
    for key in shared.catalog.apps.keys() {
        stats.entry(key.clone()).or_insert_with(|| {
            json!({ "key": key, "today": 0, "week": 0, "total": 0, "open": 0, "launches": 0, "days": 0, "lastUsed": null })
        });
    }

    let mut list: Vec<Value> = stats.into_values().collect();
    list.sort_by_key(|app| std::cmp::Reverse(app["total"].as_u64().unwrap_or(0)));

    json!({ "apps": list, "catalogVersion": shared.catalog.version })
}

/// Fiche d'une application : 30 derniers jours et détail du jour.
pub fn app(shared: &Shared, history: &mut History, key: &str) -> Value {
    let today = today();
    let days: Vec<Value> = (0..30)
        .rev()
        .map(|offset| {
            let date = today - Duration::days(offset);
            let totals = summary(shared, history, date)
                .apps
                .get(key)
                .copied()
                .unwrap_or_default();
            json!({ "date": date_string(date), "active": totals.active, "open": totals.open })
        })
        .collect();

    let usage = shared.day.apps.get(key).cloned().unwrap_or_default();
    let spans = |list: &[super::store::Span]| -> Vec<Value> {
        list.iter()
            .filter(|span| span.app == key)
            .map(|span| json!({ "start": span.start, "end": span.end }))
            .collect()
    };

    json!({
        "key": key,
        "info": shared.catalog.apps.get(key),
        "days": days,
        "today": {
            "active": usage.active,
            "open": usage.open,
            "launches": usage.launches,
            "hours": usage.hours,
        },
        "focus": spans(&shared.day.focus),
        "sessions": spans(&shared.day.sessions),
        "runningSince": shared.live.running.get(key),
        "budget": budget_statuses(shared).into_iter().find(|status| status.budget.app == key),
        "catalogVersion": shared.catalog.version,
    })
}

pub fn catalog(shared: &Shared) -> Value {
    let apps: BTreeMap<&String, Value> = shared
        .catalog
        .apps
        .iter()
        .map(|(key, info)| {
            (
                key,
                json!({
                    "name": info.display_name(),
                    "originalName": info.name,
                    "path": info.path,
                    "icon": info.icon,
                    "hidden": info.hidden,
                }),
            )
        })
        .collect();

    json!({ "version": shared.catalog.version, "apps": apps })
}
