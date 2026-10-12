"""Notifications : chaque longueur de texte, dans les deux thèmes.

La notification ajuste sa hauteur à son contenu : ce script en affiche
plusieurs, du message d'une ligne au texte démesuré, les mesure avec
`Atelier.audit()` et les capture.

    .venv\\Scripts\\python scripts\\audit\\notices.py
"""

from __future__ import annotations

import shutil
import sys
import time
from datetime import datetime
from pathlib import Path

from playwright.sync_api import sync_playwright

sys.path.insert(0, str(Path(__file__).parent))
import audit  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]

CASES = [
    ("courte", "Budget atteint", "Il reste 5 min de Navigateur aujourd’hui.", None),
    ("moyenne", "Budget épuisé", "Table des caractères va être fermé. Enregistrez ce qui doit l’être.", 60),
    ("longue", "Budget épuisé pour une application au nom particulièrement long",
     "Application au nom particulièrement long pour éprouver la troncature va être fermée. "
     "Enregistrez ce qui doit l’être avant la fin du compte à rebours.", 600),
    ("demesuree", "OutilDeSynchronisationSansEspacesNiTiretsPourTesterLaCoupureDesMotsDansLeTitre",
     "OutilDeSynchronisationSansEspacesNiTiretsPourTesterLaCoupureDesMots va être fermé. " * 4, 3600),
]


def main() -> int:
    directory = ROOT / "audit-report" / ("notifications-" + datetime.now().strftime("%Y-%m-%d_%H-%M-%S"))
    run_dir = directory / "run"
    (run_dir / "data").mkdir(parents=True)
    shutil.copy2(ROOT / "target" / "release" / "atelier.exe", run_dir / "atelier.exe")
    import scenarios
    scenarios.prepare("vide", run_dir / "data")

    total = 0
    with sync_playwright() as playwright:
        process = audit.launch(run_dir, 9533)
        try:
            browser = audit.connect(playwright, 9533)
            shell = audit.pages_by_kind(browser)["shell"][0]
            audit.settle(shell)
            for name, title, body, countdown in CASES:
                shell.evaluate(
                    "([title, body, countdown]) => Atelier.request('core', 'notice.preview', { title, body, countdown })",
                    [title, body, countdown],
                )
                deadline = time.time() + 10
                notices = []
                while not notices and time.time() < deadline:
                    time.sleep(0.4)
                    notices = audit.pages_by_kind(browser, wait=0).get("notice", [])
                page = notices[0]
                for theme in audit.THEMES:
                    audit.set_theme(page, theme)
                    audit.settle(page)
                    issues = page.evaluate("Atelier.audit()")
                    size = page.evaluate("[innerWidth, innerHeight]")
                    path = directory / f"{name}-{theme}.png"
                    page.screenshot(path=str(path))
                    total += len(issues)
                    print(f"{name:10} {theme:6} fenêtre {size[0]}x{size[1]} : {len(issues)} défaut(s) -> {path.name}")
                    for issue in issues:
                        print(f"    [{issue['kind']}] {issue['element']} {issue.get('detail', '')}")
            browser.close()
        finally:
            audit.stop(process)
    print(f"Captures : {directory}")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
