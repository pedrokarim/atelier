r"""Construit le logo d'Atelier et ses icônes.

Tout est calculé ici, rien n'est dessiné à la main : symbole, coupe pour les
petites tailles, nom en tracés (Geist, instanciée en graisse 600), logos
horizontal et empilé, icône d'application, en couleur, noir, blanc et sur fond
sombre. Les SVG vont dans `assets/logo/` ; les icônes de l'exécutable
(`assets/atelier.ico`) et de la zone de notification (`assets/tray-*.png`)
sont rendues par Edge, déjà présent sur Windows, à travers Playwright.

    .venv\Scripts\python -m pip install -r scripts\logo\requirements.txt
    .venv\Scripts\python scripts\logo\build_logo.py

Le guide d'usage est dans `assets/logo/README.md`.
"""

from __future__ import annotations

from pathlib import Path

from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "assets"
OUT = ASSETS / "logo"
FONT = ASSETS / "fonts" / "Geist-Variable.woff2"

# Tailles de l'icône de l'exécutable. De 16 à 24 px, des dessins au pixel près ;
# au-delà, l'icône d'application vectorielle.
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
# Zone de notification : 16 px à 100 %, 20 à 125 %, 24 à 150 %, 32 à 200 %.
TRAY_SIZES = [16, 20, 24, 32]

# --- Palette -----------------------------------------------------------------

BLUE = "#2a78d6"        # bleu d'Atelier (--accent du thème clair)
BLUE_DARK = "#3987e5"   # même bleu, réglé pour les fonds sombres
SLOT = "#8fb9ec"        # la case libre : le bleu éclairci, comme la case claire de l'ancienne icône
SLOT_ON_BLUE = "#a9cbf4"
INK = "#0b0b0b"         # texte d'Atelier
NIGHT = "#111110"       # fond sombre d'Atelier

# --- Symbole -------------------------------------------------------------------
#
# Grille de 3 × 3 sur un carré de 208 : cases de 58, gouttières de 17. Le grand
# widget occupe 2 × 2 cases, le moyen 2 × 1, les petits 1 × 1 ; la neuvième
# case reste libre et porte un « + ». Rayons : 16 pour le grand, 12 pour les
# autres (un rayon identique paraîtrait plus petit sur la grande forme).

CELL, GUTTER, ORIGIN = 58, 17, 24
SPAN = 2 * CELL + GUTTER  # 133


def tiles(inset: float = 0.0) -> list[tuple[float, float, float, float, float]]:
    """(x, y, largeur, hauteur, rayon). `inset` amincit les formes de la
    version inversée, qui paraît sinon plus grasse (irradiation)."""
    second = ORIGIN + SPAN + GUTTER  # 174
    shapes = [
        (ORIGIN, ORIGIN, SPAN, SPAN, 16),
        (second, ORIGIN, CELL, CELL, 12),
        (second, ORIGIN + CELL + GUTTER, CELL, CELL, 12),
        (ORIGIN, second, SPAN, CELL, 12),
    ]
    return [(x + inset, y + inset, w - 2 * inset, h - 2 * inset, max(0, r - inset)) for x, y, w, h, r in shapes]


def plus(inset: float = 0.0) -> list[tuple[float, float, float, float, float]]:
    """La croix de la case libre : branches de 44 sur 14, légèrement plus
    épaisse que le calcul ne le voudrait, pour survivre en 24 px."""
    centre = ORIGIN + SPAN + GUTTER + CELL / 2  # 203
    length, weight = 44 - 2 * inset, 14 - 2 * inset
    return [
        (centre - length / 2, centre - weight / 2, length, weight, 3),
        (centre - weight / 2, centre - length / 2, weight, length, 3),
    ]


def rects(shapes, fill: str) -> str:
    return "".join(
        f'<rect x="{x:g}" y="{y:g}" width="{w:g}" height="{h:g}" rx="{r:g}" fill="{fill}"/>'
        for x, y, w, h, r in shapes
    )


def symbol_body(tile_fill: str, slot_fill: str, inset: float = 0.0, dx: float = 0, dy: float = 0) -> str:
    body = rects(tiles(inset), tile_fill) + rects(plus(inset), slot_fill)
    return f'<g transform="translate({dx:g} {dy:g})">{body}</g>' if dx or dy else body


def small_body(tile_fill: str, slot_fill: str) -> str:
    """Coupe pour 16 à 32 px, dessinée sur la grille des pixels de 16 × 16 puis
    agrandie 16 fois : cases de 4, gouttières de 1, rien de plus fin qu'un pixel."""
    unit = 16
    shapes = [(1, 1, 9, 9, 1.5), (11, 1, 4, 4, 1), (11, 6, 4, 4, 1), (1, 11, 9, 4, 1)]
    # Croix sans arrondi : à 16 px, un arrondi d'un demi-pixel la rendait floue.
    cross = [(11, 12, 4, 2, 0), (12, 11, 2, 4, 0)]
    scale = lambda items: [(x * unit, y * unit, w * unit, h * unit, r * unit) for x, y, w, h, r in items]
    return rects(scale(shapes), tile_fill) + rects(scale(cross), slot_fill)


def svg(width: float, height: float, title: str, body: str, background: str | None = None) -> str:
    back = f'<rect width="{width:g}" height="{height:g}" fill="{background}"/>' if background else ""
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:g} {height:g}">'
            f"<title>{title}</title>{back}{body}</svg>\n")


# --- Nom en tracés ---------------------------------------------------------------

def load_font() -> TTFont:
    font = TTFont(str(FONT))
    return instancer.instantiateVariableFont(font, {"wght": 600})


def pair_kerning(font: TTFont) -> dict[tuple[str, str], int]:
    """Lit l'approche de paire (GPOS, type 2) : assez pour un mot de sept lettres."""
    kerning: dict[tuple[str, str], int] = {}
    if "GPOS" not in font:
        return kerning
    table = font["GPOS"].table
    for lookup in table.LookupList.Lookup:
        for sub in lookup.SubTable:
            kind = lookup.LookupType
            # Sous-table d'extension (type 9) : la vraie est à l'intérieur.
            if kind == 9:
                kind = sub.ExtensionLookupType
                sub = sub.ExtSubTable
            if kind != 2:
                continue
            first = sub.Coverage.glyphs
            if sub.Format == 1:
                for index, glyph in enumerate(first):
                    for record in sub.PairSet[index].PairValueRecord:
                        value = record.Value1
                        if value is not None and getattr(value, "XAdvance", 0):
                            kerning.setdefault((glyph, record.SecondGlyph), value.XAdvance)
            elif sub.Format == 2:
                class1 = sub.ClassDef1.classDefs
                class2 = sub.ClassDef2.classDefs
                for glyph in first:
                    c1 = class1.get(glyph, 0)
                    for second, c2 in list(class2.items()):
                        record = sub.Class1Record[c1].Class2Record[c2]
                        value = record.Value1
                        if value is not None and getattr(value, "XAdvance", 0):
                            kerning.setdefault((glyph, second), value.XAdvance)
    return kerning


def wordmark(font: TTFont, text: str, cap_height: float, tracking: float = -0.012):
    """Tracés du mot, à la hauteur de capitale voulue. Renvoie (tracés, largeur,
    hauteur de capitale réelle, descente utile)."""
    units = font["head"].unitsPerEm
    caps = font["OS/2"].sCapHeight
    scale = cap_height / caps
    cmap = font.getBestCmap()
    glyphs = font.getGlyphSet()
    kerning = pair_kerning(font)
    names = [cmap[ord(character)] for character in text]

    paths, cursor = [], 0.0
    left_edge = None
    right_edge = 0.0
    for index, name in enumerate(names):
        pen = SVGPathPen(glyphs)
        # Ordonnée inversée : la ligne de base est à y = 0, les capitales en négatif.
        glyphs[name].draw(TransformPen(pen, (scale, 0, 0, -scale, cursor, 0)))
        bounds = BoundsPen(glyphs)
        glyphs[name].draw(TransformPen(bounds, (scale, 0, 0, -scale, cursor, 0)))
        if bounds.bounds:
            x0, _, x1, _ = bounds.bounds
            left_edge = x0 if left_edge is None else min(left_edge, x0)
            right_edge = max(right_edge, x1)
        paths.append(pen.getCommands())
        advance = glyphs[name].width
        if index + 1 < len(names):
            advance += kerning.get((name, names[index + 1]), 0)
        cursor += advance * scale + tracking * units * scale
    return " ".join(paths), left_edge or 0.0, right_edge


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    font = load_font()

    # Symbole : couleur, noir, blanc (aminci), sombre.
    symbol = {
        "atelier-symbol": symbol_body(BLUE, SLOT),
        "atelier-symbol-black": symbol_body("#000000", "#000000"),
        "atelier-symbol-white": symbol_body("#ffffff", "#ffffff", inset=1),
        "atelier-symbol-on-dark": symbol_body(BLUE_DARK, "#6da7ec", inset=1),
    }
    for name, body in symbol.items():
        (OUT / f"{name}.svg").write_text(svg(256, 256, "Atelier", body), encoding="utf-8", newline="\n")
    (OUT / "atelier-symbol-small.svg").write_text(svg(256, 256, "Atelier", small_body("#000000", "#000000")), encoding="utf-8", newline="\n")
    (OUT / "atelier-symbol-small-color.svg").write_text(svg(256, 256, "Atelier", small_body(BLUE, SLOT)), encoding="utf-8", newline="\n")

    # Icône d'application : symbole blanc aminci sur la tuile bleue, centré
    # optiquement (2 % plus haut).
    def app_icon(body_fn, scale=0.62, lift=0.02, size=256):
        tile = f'<rect width="{size}" height="{size}" rx="{size * 0.22:g}" fill="{BLUE}"/>'
        offset = size * (1 - scale) / 2
        inner = f'<g transform="translate({offset:g} {offset - size * lift:g}) scale({scale * size / 256:g})">{body_fn()}</g>'
        return svg(size, size, "Atelier", tile + inner)

    (OUT / "atelier-app-icon.svg").write_text(app_icon(lambda: symbol_body("#ffffff", SLOT_ON_BLUE, inset=1)), encoding="utf-8", newline="\n")
    # De 16 à 32 px, la coupe pour petites tailles remplit la tuile : sa grille
    # de 16 garde une marge bleue d'un pixel, et chaque forme tombe sur les pixels.
    (OUT / "atelier-app-icon-small.svg").write_text(
        app_icon(lambda: small_body("#ffffff", SLOT_ON_BLUE), scale=1, lift=0), encoding="utf-8", newline="\n")

    # Nom : hauteur de capitale = 0,55 × la hauteur du symbole (208).
    mark_height = 208
    cap = round(mark_height * 0.55)
    word_paths, word_left, word_right = wordmark(font, "Atelier", cap)
    word_width = word_right - word_left

    def word(fill: str, x: float, baseline: float) -> str:
        return f'<path transform="translate({x - word_left:g} {baseline:g})" fill="{fill}" d="{word_paths}"/>'

    (OUT / "atelier-wordmark.svg").write_text(
        svg(word_width + 8, cap + 8 + cap * 0.02, "Atelier", word(INK, 4, cap + 4)), encoding="utf-8", newline="\n")

    # Horizontal : écart symbole–nom = 0,75 × un petit widget (44).
    gap = round(CELL * 0.75)
    width = mark_height + gap + word_width
    baseline = mark_height / 2 + cap / 2

    def horizontal(tile_fill, slot_fill, text_fill, inset=0, background=None):
        body = symbol_body(tile_fill, slot_fill, inset, dx=-ORIGIN, dy=-ORIGIN) + word(text_fill, mark_height + gap, baseline)
        return svg(width, mark_height, "Atelier", body, background)

    (OUT / "atelier-horizontal.svg").write_text(horizontal(BLUE, SLOT, INK), encoding="utf-8", newline="\n")
    (OUT / "atelier-horizontal-black.svg").write_text(horizontal("#000000", "#000000", "#000000"), encoding="utf-8", newline="\n")
    (OUT / "atelier-horizontal-white.svg").write_text(horizontal("#ffffff", "#ffffff", "#ffffff", inset=1), encoding="utf-8", newline="\n")
    (OUT / "atelier-horizontal-on-dark.svg").write_text(
        horizontal(BLUE_DARK, "#6da7ec", "#f0efec", inset=1), encoding="utf-8", newline="\n")

    # Empilé : nom centré sous le symbole, à la même distance.
    stacked_cap = round(mark_height * 0.34)
    stack_paths, stack_left, stack_right = wordmark(font, "Atelier", stacked_cap)
    stack_width = stack_right - stack_left
    total_width = max(mark_height, stack_width)
    total_height = mark_height + gap + stacked_cap

    def stacked(tile_fill, slot_fill, text_fill, inset=0):
        mark_x = (total_width - mark_height) / 2
        text_x = (total_width - stack_width) / 2
        body = symbol_body(tile_fill, slot_fill, inset, dx=mark_x - ORIGIN, dy=-ORIGIN)
        body += f'<path transform="translate({text_x - stack_left:g} {total_height:g})" fill="{text_fill}" d="{stack_paths}"/>'
        return svg(total_width, total_height, "Atelier", body)

    (OUT / "atelier-stacked.svg").write_text(stacked(BLUE, SLOT, INK), encoding="utf-8", newline="\n")
    (OUT / "atelier-stacked-black.svg").write_text(stacked("#000000", "#000000", "#000000"), encoding="utf-8", newline="\n")
    (OUT / "atelier-stacked-white.svg").write_text(stacked("#ffffff", "#ffffff", "#ffffff", inset=1), encoding="utf-8", newline="\n")

    # Icônes au pixel près pour la zone de notification et la barre des tâches :
    # 16 px (100 %), 20 px (125 %), 24 px (150 %). Coordonnées en pixels.
    for size in (16, 20, 24):
        if size == 16:
            # Dessin dédié sur 12 pixels, marge bleue de 2.
            shapes = [(2, 2, 7, 7, 1), (10, 2, 4, 3, 0.5), (10, 6, 4, 3, 0.5), (2, 10, 7, 4, 0.5)]
            cross = [(10, 11, 4, 2, 0), (11, 10, 2, 4, 0)]
        else:
            # La coupe pour petites tailles (14 pixels), centrée sur un pixel entier.
            o = (size - 14) // 2 - 1
            shapes = [(1 + o, 1 + o, 9, 9, 1.5), (11 + o, 1 + o, 4, 4, 1), (11 + o, 6 + o, 4, 4, 1), (1 + o, 11 + o, 9, 4, 1)]
            cross = [(11 + o, 12 + o, 4, 2, 0), (12 + o, 11 + o, 2, 4, 0)]
        tile = f'<rect width="{size}" height="{size}" rx="{size * 0.22:g}" fill="{BLUE}"/>'
        (OUT / f"atelier-app-icon-{size}px.svg").write_text(
            svg(size, size, "Atelier", tile + rects(shapes, "#ffffff") + rects(cross, SLOT_ON_BLUE)), encoding="utf-8", newline="\n")

    render_icons()
    print(f"logo écrit dans {OUT}, icônes dans {ASSETS}")


def icon_source(size: int) -> Path:
    if size in (16, 20, 24):
        return OUT / f"atelier-app-icon-{size}px.svg"
    return OUT / "atelier-app-icon.svg"


def render_icons() -> None:
    """Rend chaque taille d'icône en PNG transparent, puis assemble l'icône
    Windows : une image par taille, jamais une réduction de la grande."""
    import io

    from PIL import Image
    from playwright.sync_api import sync_playwright

    images: dict[int, bytes] = {}
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(channel="msedge")
        page = browser.new_page(device_scale_factor=1)
        for size in sorted(set(ICO_SIZES + TRAY_SIZES)):
            markup = icon_source(size).read_text(encoding="utf-8")
            sized = markup.replace("<svg ", f'<svg width="{size}" height="{size}" ', 1)
            page.set_viewport_size({"width": size, "height": size})
            page.set_content(f"<html><body style='margin:0;background:transparent'>{sized}</body></html>")
            images[size] = page.screenshot(omit_background=True, clip={"x": 0, "y": 0, "width": size, "height": size})
        browser.close()

    for size in TRAY_SIZES:
        (ASSETS / f"tray-{size}.png").write_bytes(images[size])

    frames = [Image.open(io.BytesIO(images[size])).convert("RGBA") for size in ICO_SIZES]
    frames[-1].save(ASSETS / "atelier.ico", format="ICO", sizes=[(size, size) for size in ICO_SIZES],
                    append_images=frames[:-1])


if __name__ == "__main__":
    main()
