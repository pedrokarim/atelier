# Atelier – guide d’usage du logo

## 1. Le logo

- **Idée** : les vraies tailles de widgets d’Atelier (un grand, deux petits, un moyen) posées sur leur grille, et la neuvième case laissée libre avec un « + » : la place qui attend votre prochain widget.
- **Versions** : horizontale (principale) · empilée · symbole seul · icône d’application.
- **Fichiers** :

| Fichier | Contenu |
| --- | --- |
| `atelier-symbol*.svg` | symbole : couleur, noir, blanc, fond sombre, coupe pour petites tailles |
| `atelier-horizontal*.svg` | logo horizontal (principal) : couleur, noir, blanc, fond sombre |
| `atelier-stacked*.svg` | logo empilé : couleur, noir, blanc |
| `atelier-wordmark.svg` | le nom seul |
| `atelier-app-icon.svg` | icône d'application |
| `atelier-app-icon-16px.svg`, `-20px`, `-24px` | icônes dessinées au pixel près |
| `../atelier.ico` | icône de l'exécutable et de l'installateur (16 à 256 px) |
| `../tray-*.png` | icône de la zone de notification, une par échelle d'affichage |

## 2. Construction

Grille de 3 × 3 sur un carré de 208 (dans une boîte de 256) : cases de **58**, gouttières de **17**. Le grand widget occupe 2 × 2 cases, le moyen 2 × 1. Rayons : 16 pour le grand, 12 pour les autres. Croix de la case libre : branches de 44 sur 14.

Le nom est en **Geist SemiBold** (600), la police de l’interface, converti en tracés ; hauteur de capitale : 0,55 × la hauteur du symbole. Écart entre le symbole et le nom : 0,75 × une petite case.

## 3. Zone de protection

Autour du logo, une marge libre d’**une petite case** (58 sur 256, soit 23 % de la hauteur du symbole) de chaque côté. Elle suit la taille du logo.

## 4. Tailles minimales

| Version | Écran |
| --- | --- |
| Horizontale | 96 px de large |
| Empilée | 64 px de large |
| Symbole | 32 px ; en dessous, les dessins au pixel près |
| Icône 16, 20, 24 px | `atelier-app-icon-16px.svg`, `-20px`, `-24px` : à utiliser telles quelles, jamais réduites depuis la grande |

## 5. Couleurs

| Nom | HEX | RGB | Usage |
| --- | --- | --- | --- |
| Bleu Atelier | `#2a78d6` | 42, 120, 214 | symbole, tuile de l’icône |
| Bleu Atelier, fond sombre | `#3987e5` | 57, 135, 229 | symbole sur `#111110` |
| Case libre | `#8fb9ec` | 143, 185, 236 | la croix, sur fond clair |
| Case libre sur bleu | `#a9cbf4` | 169, 203, 244 | la croix, dans l’icône |
| Encre | `#0b0b0b` | 11, 11, 11 | le nom, sur fond clair |

Le bleu et l’encre reprennent les couleurs de l’interface d’Atelier (`--accent`, `--text`). Les teintes d’impression (CMJN, Pantone) sont à fixer avec l’imprimeur si un jour il y en a besoin.

**Associations validées** : couleur sur blanc · version « fond sombre » sur `#111110` · blanc sur bleu Atelier · noir sur blanc. Sur une photo : la version blanche ou noire, sur une zone calme.

## 6. À ne pas faire

Ne pas déformer, pivoter ni recolorer hors de la palette. Ne pas ajouter d’ombre, de contour, de dégradé ou d’effet. Ne pas déplacer ni redimensionner les parties d’un logo complet. Ne pas retaper le nom avec une police : utiliser les fichiers. Ne pas réduire la grande icône pour obtenir une petite : prendre le dessin de la taille voulue.

## 7. Régénérer les fichiers

```sh
.venv\Scripts\python -m pip install -r scripts\logo\requirements.txt
.venv\Scripts\python scripts\logo\build_logo.py
```

Tous les SVG de ce dossier, `atelier.ico` et les `tray-*.png` sont recalculés. Le rendu des PNG passe par Edge, déjà présent sur Windows.
