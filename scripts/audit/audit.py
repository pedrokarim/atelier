"""Audit de mise en page d'Atelier.

Lance le vrai programme, en mode portable avec un jeu de données choisi, et
s'y branche avec Playwright par le port de débogage de WebView2. Chaque écran
est mesuré dans la page (`Atelier.audit()`) et capturé :

- chaque page, chaque boîte de dialogue, le laboratoire des widgets (chaque
  widget dans chacune de ses tailles), les widgets épinglés sur le bureau et
  la notification ;
- en thème sombre et clair ;
- à plusieurs largeurs de fenêtre ;
- avec des données vides, normales et extrêmes.

Le rapport (`audit-report/<date>/index.html`) liste chaque défaut avec sa
capture. Le code de sortie vaut 1 s'il en reste un.

Usage, depuis la racine du dépôt, après `cargo build --release` :

    python -m venv .venv
    .venv\\Scripts\\python -m pip install -r scripts\\audit\\requirements.txt
    .venv\\Scripts\\python scripts\\audit\\audit.py
    .venv\\Scripts\\python scripts\\audit\\audit.py --scenario extreme --size 1024x768
"""

from __future__ import annotations

import argparse
import html
import json
import os
import shutil
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

from playwright.sync_api import Page, sync_playwright

sys.path.insert(0, str(Path(__file__).parent))
import scenarios  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
BASE_PORT = 9333
SCENARIOS = ["vide", "demo", "extreme"]
THEMES = ["sombre", "clair"]
# Largeur minimale de la fenêtre principale : 860 pixels logiques.
SIZES = [(1920, 1080), (1440, 900), (1280, 800), (1024, 768), (860, 600)]


def routes(scenario: str) -> list[str]:
    pages = [
        "home",
        "lab",
        "screen-time/today",
        "screen-time/history?days=7",
        "screen-time/history?days=30",
        "screen-time/history?days=90",
        "screen-time/apps",
        "screen-time/budgets",
        "screen-time/settings",
        "system/overview",
        "system/processes",
        "system/processes?focus=c%3A%5Caudit%5Cnavigateur.exe",
        "settings",
    ]
    key = scenarios.first_app_key(scenario)
    if key:
        pages.insert(7, "screen-time/app?" + "key=" + key.replace("\\", "%5C").replace(":", "%3A").replace(" ", "%20"))
    return pages


# Boîtes de dialogue : page, bouton qui l'ouvre.
DIALOGS = [
    ("home", "#addWidget", "catalogue des widgets"),
    ("screen-time/budgets", "#add", "nouveau budget"),
    ("system/overview", "[data-stop]", "arrêt d'un processus"),
    ("system/processes", "[data-group-menu]", "menu d'une application"),
]


def launch(run_dir: Path, port: int) -> subprocess.Popen:
    env = dict(os.environ)
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = f"--remote-debugging-port={port}"
    return subprocess.Popen([str(run_dir / "atelier.exe"), "--open", "home"], cwd=run_dir, env=env)


def stop(process: subprocess.Popen) -> None:
    """Arrête le programme et les processus WebView2 qu'il a lancés : ils
    garderaient sinon le port de débogage ouvert."""
    subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True)
    process.wait()


def connect(playwright, port: int, timeout: float = 30):
    deadline = time.time() + timeout
    while True:
        try:
            return playwright.chromium.connect_over_cdp(f"http://127.0.0.1:{port}")
        except Exception:
            if time.time() > deadline:
                raise
            time.sleep(0.5)


def kind_of(page: Page) -> str | None:
    try:
        return page.evaluate("window.ATELIER_BOOT ? ATELIER_BOOT.kind : null")
    except Exception:
        return None


def pages_by_kind(browser, wait: float = 45) -> dict[str, list[Page]]:
    # Attente large : chaque widget épinglé ouvre sa propre vue web, et le
    # scénario « extreme » en compte une dizaine. On rend la main dès que la
    # fenêtre principale est là.
    deadline = time.time() + wait
    while True:
        found: dict[str, list[Page]] = {}
        for context in browser.contexts:
            for page in context.pages:
                kind = kind_of(page)
                if kind:
                    found.setdefault(kind, []).append(page)
        if "shell" in found or time.time() > deadline:
            return found
        time.sleep(0.5)


def settle(page: Page) -> None:
    """Attend la fin des chargements : plus aucun squelette à l'écran."""
    page.wait_for_timeout(500)
    for _ in range(30):
        if not page.evaluate("document.querySelector('.skeleton') !== null"):
            break
        page.wait_for_timeout(100)
    page.wait_for_timeout(300)


def set_theme(page: Page, theme: str) -> None:
    page.evaluate(f"Atelier.setTheme({'true' if theme == 'sombre' else 'false'})")


class Report:
    def __init__(self, directory: Path):
        self.directory = directory
        self.shots = directory / "captures"
        self.shots.mkdir(parents=True, exist_ok=True)
        self.screens: list[dict] = []

    def record(self, page: Page, scenario: str, theme: str, size: str, screen: str) -> list[dict]:
        issues = page.evaluate("Atelier.audit()")
        name = f"{scenario}-{theme}-{size}-{screen}".replace("/", "_").replace("?", "_").replace("=", "-")
        name = "".join(character for character in name if character.isalnum() or character in "-_.")[:120]
        shot = self.shots / f"{name}.png"
        # La fenêtre telle qu'on la voit : le contenu défile dans la page, pas
        # le document.
        page.screenshot(path=str(shot))
        self.screens.append({
            "scenario": scenario, "theme": theme, "size": size, "screen": screen,
            "capture": f"captures/{shot.name}", "issues": issues,
        })
        status = "ok" if not issues else f"{len(issues)} défaut(s)"
        print(f"  {scenario:8} {theme:6} {size:10} {screen:42} {status}")
        return issues

    def write(self) -> int:
        total = sum(len(screen["issues"]) for screen in self.screens)
        (self.directory / "report.json").write_text(json.dumps(self.screens, ensure_ascii=False, indent=2), encoding="utf-8")

        rows = []
        for screen in sorted(self.screens, key=lambda item: (-len(item["issues"]), item["screen"])):
            issues = "".join(
                f"<li><b>{html.escape(issue['kind'])}</b> – {html.escape(issue['element'])}"
                f"{' – ' + html.escape(issue['detail']) if issue.get('detail') else ''}"
                f"{'<br><small>avec ' + html.escape(issue['other']) + '</small>' if issue.get('other') else ''}</li>"
                for issue in screen["issues"]
            )
            rows.append(f"""
              <article class="{'bad' if screen['issues'] else 'good'}">
                <header>{html.escape(screen['screen'])}<span>{html.escape(screen['scenario'])} · {html.escape(screen['theme'])} · {html.escape(screen['size'])}</span></header>
                {'<ul>' + issues + '</ul>' if issues else '<p>Aucun défaut mesuré.</p>'}
                <a href="{screen['capture']}"><img src="{screen['capture']}" loading="lazy" alt=""></a>
              </article>""")

        (self.directory / "index.html").write_text(f"""<!doctype html>
<html lang="fr"><head><meta charset="utf-8"><title>Audit de mise en page</title>
<style>
  body {{ margin: 0; padding: 24px; background: #111110; color: #f0efec; font: 14px system-ui, sans-serif; }}
  h1 {{ margin: 0 0 4px; }} p.summary {{ color: #c3c2b7; margin: 0 0 20px; }}
  main {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 420px), 1fr)); gap: 16px; }}
  article {{ min-width: 0; padding: 14px; border-radius: 12px; background: #1a1a19; border: 1px solid #2c2c2a; }}
  article.bad {{ border-color: #d03b3b; }}
  header {{ font-weight: 650; overflow-wrap: anywhere; }} header span {{ display: block; font-weight: 400; color: #8f8d86; font-size: 12px; }}
  ul {{ padding-left: 18px; color: #f07a7a; overflow-wrap: anywhere; }} p {{ color: #3bc43b; }}
  img {{ display: block; max-width: 100%; margin-top: 10px; border-radius: 8px; }}
</style></head><body>
<h1>Audit de mise en page</h1>
<p class="summary">{len(self.screens)} écrans mesurés, {total} défaut(s), le {datetime.now():%d/%m/%Y à %H:%M}.</p>
<main>{''.join(rows)}</main>
</body></html>""", encoding="utf-8")
        return total


def audit_scenario(playwright, scenario: str, themes: list[str], sizes: list[tuple[int, int]], report: Report, exe: Path, port: int) -> None:
    run_dir = report.directory / "run" / scenario
    shutil.rmtree(run_dir, ignore_errors=True)
    (run_dir / "data").mkdir(parents=True)
    shutil.copy2(exe, run_dir / "atelier.exe")
    scenarios.prepare(scenario, run_dir / "data")

    process = launch(run_dir, port)
    try:
        browser = connect(playwright, port)
        found = pages_by_kind(browser)
        if "shell" not in found:
            raise RuntimeError(f"fenêtre principale introuvable sur le port {port}")
        shell = found["shell"][0]
        settle(shell)

        # Notification d'essai aux textes longs, pour une mesure déterministe.
        shell.evaluate("""Atelier.request('core', 'notice.preview', {
            title: 'Budget épuisé pour une application au nom particulièrement long',
            body: 'Application au nom particulièrement long pour éprouver la troncature va être fermée. Enregistrez ce qui doit l’être avant la fin du compte à rebours.',
            countdown: 600,
        })""")
        deadline = time.time() + 10
        notices: list[Page] = []
        while not notices and time.time() < deadline:
            time.sleep(0.5)
            notices = pages_by_kind(browser, wait=0).get("notice", [])
        for page in notices:
            for theme in themes:
                set_theme(page, theme)
                settle(page)
                report.record(page, scenario, theme, "bureau", "notification")
        shell.evaluate("Atelier.request('core', 'notice.dismiss', { id: 'preview' })")

        for theme in themes:
            for width, height in sizes:
                size = f"{width}x{height}"
                shell.evaluate(f"Atelier.request('core', 'window.resize', {{ width: {width}, height: {height} }})")
                shell.wait_for_timeout(300)
                set_theme(shell, theme)
                for route in routes(scenario):
                    shell.evaluate(f"Atelier.navigate({json.dumps(route)})")
                    settle(shell)
                    report.record(shell, scenario, theme, size, route)
                for route, button, label in DIALOGS:
                    shell.evaluate(f"Atelier.navigate({json.dumps(route)})")
                    settle(shell)
                    if shell.locator(button).count():
                        shell.locator(button).first.click()
                        shell.wait_for_timeout(400)
                        report.record(shell, scenario, theme, size, f"{route} + {label}")
                        shell.keyboard.press("Escape")
                        shell.wait_for_timeout(200)

            # Widgets épinglés et notification : leur taille est fixe.
            found = pages_by_kind(browser, wait=0)
            for index, page in enumerate(found.get("widget", [])):
                set_theme(page, theme)
                settle(page)
                widget = page.evaluate("ATELIER_BOOT.instance.widget + ' (' + ATELIER_BOOT.instance.size + ')'")
                report.record(page, scenario, theme, "bureau", f"widget {widget} #{index}")
        browser.close()
    finally:
        stop(process)


def main() -> int:
    parser = argparse.ArgumentParser(description="Audit de mise en page d'Atelier")
    parser.add_argument("--exe", type=Path, default=ROOT / "target" / "release" / "atelier.exe")
    parser.add_argument("--scenario", choices=SCENARIOS, action="append")
    parser.add_argument("--theme", choices=THEMES, action="append")
    parser.add_argument("--size", action="append", help="largeur x hauteur, par exemple 1024x768")
    parser.add_argument("--out", type=Path, default=ROOT / "audit-report")
    args = parser.parse_args()

    if not args.exe.is_file():
        print(f"Exécutable introuvable : {args.exe}. Lancer d'abord cargo build --release.")
        return 2

    sizes = [tuple(int(part) for part in size.split("x")) for size in args.size] if args.size else SIZES
    directory = args.out / datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
    report = Report(directory)

    with sync_playwright() as playwright:
        # Un port par scénario : celui du précédent peut rester occupé
        # quelques instants après son arrêt.
        for index, scenario in enumerate(args.scenario or SCENARIOS):
            print(f"Scénario « {scenario} »")
            audit_scenario(playwright, scenario, args.theme or THEMES, sizes, report, args.exe, BASE_PORT + index)

    total = report.write()
    print(f"\n{len(report.screens)} écrans, {total} défaut(s). Rapport : {directory / 'index.html'}")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
