"""Jeux de données de l'audit de mise en page.

Chaque scénario remplit un dossier `data` (mode portable) :

- « vide » : premier lancement, rien d'enregistré ;
- « demo » : deux mois d'usage réaliste ;
- « extreme » : noms très longs, beaucoup d'applications, budgets dépassés,
  durées énormes.

Les applications sont fictives (`c:\\audit\\…`) : aucun budget ne peut viser
une application réellement lancée, donc aucune n'est fermée pendant l'audit.

Le module Système lit, à la place des vrais processus, le relevé fictif
`modules/system/demo.json` : ses pages sont mesurées sur des données
déterministes, et aucune de ses actions ne vise un processus réel.
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


GIB = 1024 ** 3
MIB = 1024 ** 2


def _process(pid: int, name: str, private: float, *, parent: int = 1, handles: int = 900,
             windowed: bool = False, console: bool = False, cpu: float = 0.0, hours: float = 2,
             session: int = 1, path: str | None = None, threads: int = 14) -> dict:
    now = int(datetime.now().timestamp() * 1000)
    return {
        "pid": pid, "parent": parent, "name": name,
        "path": path or "C:\\Audit\\" + name,
        "started": now - int(hours * 3_600_000),
        "privateBytes": int(private), "commit": int(private * 1.3),
        "handles": handles, "threads": threads, "session": session,
        "windowed": windowed, "console": console, "cpu": cpu,
    }


def _system(scenario: str) -> dict:
    """Relevé fictif du module Système."""
    # Le lanceur commun : parent vivant de tous les processus ordinaires.
    processes = [_process(1, "explorer.exe", 180 * MIB, parent=0, windowed=True, hours=30,
                          path="C:\\Windows\\explorer.exe")]
    processes += [_process(2 + index, "svchost.exe", 12 * MIB, parent=0, session=0,
                           path="C:\\Windows\\System32\\svchost.exe") for index in range(6)]
    memory = {
        "total": 32 * GIB, "available": 14 * GIB, "commit": 21 * GIB, "commitLimit": 61 * GIB,
        "kernelPaged": int(0.7 * GIB), "kernelNonpaged": int(0.4 * GIB), "cache": 6 * GIB,
        "handles": 210_000, "processes": 312, "threads": 4_600,
    }
    if scenario == "vide":
        processes += [_process(20, "navigateur.exe", 900 * MIB, windowed=True)]
        return {"memory": memory, "processes": processes}

    if scenario == "demo":
        memory.update(available=int(2.3 * GIB), kernelPaged=int(3.9 * GIB), kernelNonpaged=int(1.4 * GIB),
                      commit=int(36.6 * GIB), handles=7_466_215)
        processes += [
            _process(30, "jeu-de-role.exe", 7.4 * GIB, windowed=True, handles=2_713, cpu=14.0,
                     path="C:\\Audit\\Jeu de rôle.exe"),
            _process(31, "find.exe", 17 * MIB, parent=777, handles=7_250_506, console=True, cpu=4.0, hours=1.5),
            _process(32, "tail.exe", 2 * MIB, parent=778, handles=123, console=True, hours=4),
            _process(33, "indexeur.exe", 160 * MIB, handles=1_400, cpu=31.0),
            _process(34, "analyseur.exe", 2.2 * GIB, handles=1_100),
        ]
        processes += [_process(40 + index, "navigateur.exe", 90 * MIB, windowed=index == 0,
                               path="C:\\Audit\\Navigateur.exe") for index in range(17)]
        processes += [_process(60 + index, "terminal.exe", 240 * MIB, windowed=True,
                               path="C:\\Audit\\Terminal.exe") for index in range(2)]
        return {"memory": memory, "processes": processes}

    # Extrême : noms démesurés, valeurs énormes, beaucoup de suspects.
    memory.update(total=2048 * GIB, available=40 * GIB, kernelPaged=700 * GIB, kernelNonpaged=180 * GIB,
                  commit=3_900 * GIB, commitLimit=4_096 * GIB, handles=987_654_321,
                  processes=4_812, threads=99_999)
    for index, name in enumerate(LONG_NAMES):
        processes.append(_process(100 + index, name + ".exe", (90 + index * 12) * GIB, windowed=index % 2 == 0,
                                  handles=95_000_000 + index, console=index % 2 == 1, cpu=99.9,
                                  parent=9_000 + index, hours=900, path="C:\\Audit\\" + name + ".exe"))
    for index in range(1, 42):
        name = f"Application numéro {index:02d}"
        for copy in range(3 if index % 4 == 0 else 1):
            processes.append(_process(200 + index * 10 + copy, name + ".exe", (index % 9 + 1) * 37 * GIB,
                                      handles=60_000 if index % 5 == 0 else 2_000, cpu=index % 7 * 9.5,
                                      console=index % 3 == 0, parent=8_000 if index % 3 == 0 else 1,
                                      path="C:\\Audit\\" + name + ".exe"))
    return {"memory": memory, "processes": processes}


def _layout(scenario: str) -> dict:
    """Quelques widgets épinglés sur le bureau, pour auditer leur fenêtre."""
    pinned = [("screen-time.today", "m"), ("screen-time.now", "s"), ("screen-time.budgets", "l"),
              ("system.memory", "m"), ("system.clutter", "m")]
    if scenario == "extreme":
        pinned += [("screen-time.running", "l"), ("screen-time.top", "m"), ("screen-time.week", "w"),
                   ("system.memory", "s"), ("system.clutter", "l"), ("system.top", "l")]
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
    system = data_dir / "modules" / "system"
    system.mkdir(parents=True, exist_ok=True)
    (system / "demo.json").write_text(json.dumps(_system(scenario)), encoding="utf-8")

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
