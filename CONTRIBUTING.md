# Contribuer à Atelier

Merci de l'intérêt ! Quelques repères pour que les contributions restent
homogènes.

## Préparer l'environnement

- Windows 10 ou 11, avec le runtime WebView2 (présent d'office sur Windows 11).
- La chaîne Rust stable : `rustup default stable`.

```sh
cargo run -- --open home
```

Pour essayer sans toucher à ses vraies données, créer `target\debug\data\` :
le binaire de développement passe alors en mode portable.

## Avant d'ouvrir une pull request

La CI exige que ces trois commandes passent :

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Toute modification de l'interface passe aussi l'**audit de mise en page**,
qui doit finir sans aucun défaut (voir le
[README](README.md#audit-de-mise-en-page)) :

```sh
cargo build --release
.venv\Scripts\python scripts\audit\audit.py
```

Elle est ensuite **comparée à la version précédente**, écran par écran
(`scripts/audit/compare.py`) : tout écart qui n'est pas une correction
voulue est une régression, même s'il ne déborde de rien.

Une interface n'est « vérifiée » que si elle l'a été dans les deux thèmes, à
toutes les largeurs et avec les trois jeux de données. Quand l'audit signale
un défaut, on corrige la cause (une taille imposée, une colonne de largeur
fixe, un `min-width: 0` manquant), pas le symptôme.

## Conventions

| Quoi | Langue |
| --- | --- |
| Identifiants : variables, fonctions, types, fichiers, clés | anglais, sans exception |
| Commentaires, documentation, textes de l'interface | français |
| Messages de commit et descriptions de pull request | français |

Dans les textes français : espace insécable avant `: ; ! ?` et `%`,
guillemets « … », apostrophe typographique dans l'interface.

Dans l'interface, aucun emoji ni caractère Unicode employé comme pictogramme
(flèches, points, coches) : les icônes viennent de la bibliothèque Lucide,
déclarée dans `ui/core.js`.

## Ajouter un module ou un widget

Le contrat est décrit dans le [README](README.md#ajouter-un-module) ; le code
de référence est `src/modules/mod.rs` côté Rust et l'en-tête de `ui/core.js`
côté page. Un module ne modifie pas le cœur : s'il en a besoin, c'est que le
contrat doit évoluer, et cela mérite une discussion dans une issue d'abord.

## Publier une version

1. Mettre à jour `version` dans `Cargo.toml`, puis `cargo build` pour
   rafraîchir `Cargo.lock`.
2. Déplacer les entrées de `[Non publié]` vers une nouvelle section de
   `CHANGELOG.md`.
3. Committer, puis pousser l'étiquette :

   ```sh
   git tag v0.2.0
   git push origin main v0.2.0
   ```

Le workflow **Release** construit l'installateur et l'archive portable, et
publie la release avec les notes du journal.
