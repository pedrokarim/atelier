"""Jeux de données de l'audit de mise en page.

Chaque scénario remplit un dossier `data` (mode portable) :

- « vide » : premier lancement, rien d'enregistré ;
- « demo » : deux mois d'usage réaliste ;
- « extreme » : noms très longs, beaucoup d'applications, budgets dépassés,
  durées énormes.

Les applications sont fictives (`c:\\audit\\…`) : aucun budget ne peut viser
une application réellement lancée, donc aucune n'est fermée pendant l'audit.
"""

from __future__ import annotations

import json
import random
from datetime import date, datetime, timedelta
from pathlib import Path

DAY = 86_400


def _key(name: str) -> str:
    return "c:\\audit\\" + name.lower().replace(" ", "-") + ".exe"


DEMO_APPS = [
    "Navigateur", "Terminal", "Messagerie", "Notes", "Wiki", "Jeu de rôle",
    "Jeu d'aventure", "Boutique de jeux", "Explorateur", "Lecteur vidéo",
]

LONG_NAMES = [
    "Application au nom particulièrement long pour éprouver la troncature",
    "Suite bureautique professionnelle édition entreprise 2026 (64 bits)",
    "OutilDeSynchronisationSansEspacesNiTiretsPourTesterLaCoupureDesMots",
    "Client de messagerie instantanée et de visioconférence d'équipe",
]


def _apps(scenario: str) -> list[str]:
    if scenario == "extreme":
        return LONG_NAMES + [f"Application numéro {index:02d}" for index in range(1, 42)]
    return DEMO_APPS


def _catalog(names: list[str]) -> dict:
    return {
        "apps": {
            _key(name): {"name": name, "path": "C:\\Audit\\" + name + ".exe"}
            for name in names
        }
    }


def _day(day: date, names: list[str], rng: random.Random, limit: int, heavy: bool, today: bool = False) -> dict:
    """Une journée : intervalles au premier plan, sessions, totaux par heure.

    La journée en cours commence à minuit et alterne vite d'application, pour
    être chargée quelle que soit l'heure de l'audit.
    """
    focus: list[dict] = []
    t = 0 if today else 7 * 3600 + rng.randint(0, 3600)
    end = 23 * 3600 + 50 * 60 if heavy else 22 * 3600 + 30 * 60
    weights = [1 / (index + 1) for index in range(len(names))]
    while t < end:
        name = rng.choices(names, weights)[0]
        length = rng.randint(1, 6) * 60 if today else rng.randint(4, 40) * 60
        focus.append({"app": _key(name), "start": t, "end": min(end, t + length)})
        t += length + (rng.randint(0, 120) if heavy or today else rng.randint(30, 900))

    focus = [
        {**span, "end": min(span["end"], limit)} for span in focus if span["start"] < limit
    ]

    apps: dict[str, dict] = {}
    for span in focus:
        usage = apps.setdefault(span["app"], {"active": 0, "open": 0, "launches": 0, "hours": [0] * 24})
        usage["active"] += span["end"] - span["start"]
        second = span["start"]
        while second < span["end"]:
            hour = second // 3600
            stop = min(span["end"], (hour + 1) * 3600)
            usage["hours"][hour] += stop - second
            second = stop

    sessions = []
    for key in apps:
        spans = [span for span in focus if span["app"] == key]
        start = max(0, spans[0]["start"] - 300)
        stop = min(limit, spans[-1]["end"] + 600)
        sessions.append({"app": key, "start": start, "end": stop})
        apps[key]["open"] = stop - start
        apps[key]["launches"] = rng.randint(40, 120) if heavy else rng.randint(1, 4)

    return {
        "date": day.isoformat(),
        "apps": apps,
        "focus": focus,
        "sessions": sorted(sessions, key=lambda span: span["start"]),
        "extensions": {},
    }


def _budgets(names: list[str], scenario: str) -> list[dict]:
    if scenario == "extreme":
        chosen = names[:12]
        minutes = [1, 5, 30, 90, 600, 1, 2, 3, 45, 120, 240, 1440]
    else:
        chosen = names[:4]
        minutes = [90, 120, 180, 360]
    budgets = []
    for index, (name, limit) in enumerate(zip(chosen, minutes)):
        budgets.append({
            "app": _key(name),
            "minutes": limit,
            "metric": "open" if index % 3 == 2 else "active",
            # Applications fictives : la fermeture ne peut rien viser de réel.
            "action": "close" if index % 2 else "notify",
            "enabled": index != 5,
        })
    return budgets


def _layout(scenario: str) -> dict:
    """Quelques widgets épinglés sur le bureau, pour auditer leur fenêtre."""
    pinned = [("screen-time.today", "m"), ("screen-time.now", "s"), ("screen-time.budgets", "l")]
    if scenario == "extreme":
        pinned += [("screen-time.running", "l"), ("screen-time.top", "m"), ("screen-time.week", "w")]
    desktop = [
        {"id": f"audit-{index}", "widget": widget, "size": size, "settings": None,
         "x": 40 + index * 30, "y": 40 + index * 30, "topmost": False}
        for index, (widget, size) in enumerate(pinned)
    ]
    return {"board": [], "desktop": desktop, "desktop_hidden": False, "initialized": False}


def prepare(scenario: str, data_dir: Path) -> None:
    """Écrit les données du scénario dans `data_dir` (vidé au préalable)."""
    module = data_dir / "modules" / "screen-time"
    (module / "days").mkdir(parents=True, exist_ok=True)
    (data_dir / "layout.json").write_text(json.dumps(_layout(scenario)), encoding="utf-8")

    if scenario == "vide":
        return

    names = _apps(scenario)
    rng = random.Random(7)
    today = date.today()
    now = datetime.now()
    seconds_now = now.hour * 3600 + now.minute * 60

    for offset in range(59, -1, -1):
        day = today - timedelta(days=offset)
        limit = max(60, seconds_now - 120) if offset == 0 else DAY
        record = _day(day, names, rng, limit, heavy=scenario == "extreme", today=offset == 0)
        (module / "days" / f"{day.isoformat()}.json").write_text(json.dumps(record), encoding="utf-8")

    (module / "apps.json").write_text(json.dumps(_catalog(names)), encoding="utf-8")
    (module / "config.json").write_text(json.dumps({
        "settings": {
            "idle_minutes": 5, "fullscreen_counts_as_active": True, "warn_minutes": 5,
            "extension_minutes": 15, "close_grace_seconds": 60, "reminder_minutes": 30,
        },
        "budgets": _budgets(names, scenario),
    }, indent=2), encoding="utf-8")


def first_app_key(scenario: str) -> str | None:
    """Une application dont la fiche mérite d'être auditée."""
    if scenario == "vide":
        return None
    return _key(_apps(scenario)[0])
