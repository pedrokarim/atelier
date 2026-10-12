"""Comparaison visuelle entre deux versions d'Atelier.

Lance les deux exécutables sur des données identiques, capture les mêmes
écrans dans les mêmes conditions (thème, taille de fenêtre), puis mesure la
différence pixel à pixel. Le rapport montre chaque écran avant et après, avec
les zones qui ont changé : un espacement modifié sans raison s'y voit
immédiatement.

À lancer avant de publier toute modification d'interface :

    .venv\\Scripts\\python scripts\\audit\\compare.py --before <ancien atelier.exe>

Seules les zones qui affichent l'instant présent (application au premier
plan, durée du jour) peuvent légitimement différer d'une exécution à l'autre.
"""

from __future__ import annotations

import argparse
import html
import shutil
import sys
from datetime import datetime
from pathlib import Path

from PIL import Image, ImageChops
from playwright.sync_api import sync_playwright

sys.path.insert(0, str(Path(__file__).parent))
import audit  # noqa: E402
import scenarios  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]


def capture(playwright, exe: Path, data: Path, out: Path, port: int, scenario: str,
            themes: list[str], sizes: list[tuple[int, int]]) -> dict[str, Path]:
    run_dir = out / "run"
    shutil.rmtree(run_dir, ignore_errors=True)
    shutil.copytree(data, run_dir / "data")
    shutil.copy2(exe, run_dir / "atelier.exe")
    shots: dict[str, Path] = {}

    process = audit.launch(run_dir, port)
    try:
        browser = audit.connect(playwright, port)
        shell = audit.pages_by_kind(browser)["shell"][0]
        audit.settle(shell)
        # La page « lab » n'existe que dans les versions récentes.
        routes = [route for route in audit.routes(scenario) if route != "lab"]
        for theme in themes:
            for width, height in sizes:
                shell.evaluate(f"Atelier.request('core', 'window.resize', {{ width: {width}, height: {height} }})")
                shell.wait_for_timeout(300)
                audit.set_theme(shell, theme)
                for route in routes:
                    shell.evaluate(f"Atelier.navigate({route!r})")
                    audit.settle(shell)
                    name = f"{theme}-{width}x{height}-{route}"
                    name = "".join(c if c.isalnum() or c in "-_." else "_" for c in name)[:110]
                    path = out / f"{name}.png"
                    shell.screenshot(path=str(path))
                    shots[name] = path
            for index, page in enumerate(audit.pages_by_kind(browser, wait=0).get("widget", [])):
                audit.set_theme(page, theme)
                audit.settle(page)
                name = f"{theme}-bureau-widget-{index}"
                path = out / f"{name}.png"
                page.screenshot(path=str(path))
                shots[name] = path
        browser.close()
    finally:
        audit.stop(process)
    return shots


def difference(before: Path, after: Path, mark: Path) -> float:
    """Part des pixels qui diffèrent ; écrit l'image « après » avec les
    différences surlignées en rouge."""
    a = Image.open(before).convert("RGB")
    b = Image.open(after).convert("RGB")
    if a.size != b.size:
        b.save(mark)
        return 1.0
    diff = ImageChops.difference(a, b).convert("L").point(lambda value: 255 if value > 24 else 0)
    changed = sum(1 for value in diff.getdata() if value)
    overlay = b.copy()
    red = Image.new("RGB", b.size, (220, 40, 40))
    overlay.paste(red, mask=diff)
    Image.blend(b, overlay, 0.65).save(mark)
    return changed / (a.size[0] * a.size[1])


def main() -> int:
    parser = argparse.ArgumentParser(description="Comparaison visuelle entre deux versions d'Atelier")
    parser.add_argument("--before", type=Path, required=True, help="exécutable de référence")
    parser.add_argument("--after", type=Path, default=ROOT / "target" / "release" / "atelier.exe")
    parser.add_argument("--scenario", choices=audit.SCENARIOS, action="append")
    parser.add_argument("--theme", choices=audit.THEMES, action="append")
    parser.add_argument("--size", action="append", help="largeur x hauteur")
    parser.add_argument("--out", type=Path, default=ROOT / "audit-report")
    args = parser.parse_args()

    sizes = [tuple(int(part) for part in size.split("x")) for size in args.size] if args.size else [(1440, 900), (1024, 768)]
    themes = args.theme or audit.THEMES
    directory = args.out / ("comparaison-" + datetime.now().strftime("%Y-%m-%d_%H-%M-%S"))
    rows = []

    with sync_playwright() as playwright:
        for index, scenario in enumerate(args.scenario or ["demo", "extreme"]):
            print(f"Scénario « {scenario} »")
            data = directory / scenario / "data"
            data.mkdir(parents=True)
            scenarios.prepare(scenario, data)
            before = capture(playwright, args.before, data, directory / scenario / "avant", 9433 + 2 * index, scenario, themes, sizes)
            after = capture(playwright, args.after, data, directory / scenario / "apres", 9434 + 2 * index, scenario, themes, sizes)
            marks = directory / scenario / "ecarts"
            marks.mkdir(parents=True, exist_ok=True)
            for name, old in before.items():
                new = after.get(name)
                if not new:
                    continue
                ratio = difference(old, new, marks / f"{name}.png")
                rows.append((ratio, scenario, name, old, new, marks / f"{name}.png"))
                print(f"  {name:70} {ratio * 100:6.2f} %")

    def rel(path: Path) -> str:
        return path.relative_to(directory).as_posix()

    rows.sort(key=lambda row: -row[0])
    cards = "".join(f"""
      <article>
        <header>{html.escape(scenario)} · {html.escape(name)}<span>{ratio * 100:.2f} % des pixels ont changé</span></header>
        <div class="trio">
          <figure><figcaption>0.1.1</figcaption><img src="{rel(old)}" loading="lazy"></figure>
          <figure><figcaption>Nouvelle version</figcaption><img src="{rel(new)}" loading="lazy"></figure>
          <figure><figcaption>Écarts en rouge</figcaption><img src="{rel(mark)}" loading="lazy"></figure>
        </div>
      </article>""" for ratio, scenario, name, old, new, mark in rows)
    (directory / "index.html").write_text(f"""<!doctype html><html lang="fr"><head><meta charset="utf-8">
<title>Comparaison visuelle</title><style>
body {{ margin: 0; padding: 24px; background: #111110; color: #f0efec; font: 14px system-ui, sans-serif; }}
article {{ margin-bottom: 28px; padding: 14px; border-radius: 12px; background: #1a1a19; border: 1px solid #2c2c2a; }}
header {{ font-weight: 650; margin-bottom: 10px; overflow-wrap: anywhere; }} header span {{ display: block; font-weight: 400; color: #8f8d86; }}
.trio {{ display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; }}
figure {{ margin: 0; min-width: 0; }} figcaption {{ color: #c3c2b7; margin-bottom: 4px; }}
img {{ display: block; max-width: 100%; border-radius: 6px; }}
</style></head><body><h1>Comparaison visuelle</h1>{cards}</body></html>""", encoding="utf-8")
    print(f"\nRapport : {directory / 'index.html'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
