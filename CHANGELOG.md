# Journal des modifications

Toutes les évolutions notables d'Atelier sont consignées ici. Le format suit
[Keep a Changelog](https://keepachangelog.com/fr/1.1.0/), et les versions
respectent le [versionnage sémantique](https://semver.org/lang/fr/).

## [Non publié]

## [0.1.0] - 2026-10-11

Première version.

### Ajouté

- Application résidente dans la zone de notification, en un seul exécutable,
  avec une fenêtre principale rendue par WebView2.
- Architecture en modules : chaque module apporte ses pages et ses widgets.
- Widgets en quatre tailles, posés sur la grille d'accueil (ajout,
  réorganisation par glisser-déposer) ou épinglés sur le bureau comme petites
  fenêtres flottantes.
- Notifications dans le coin de l'écran, avec boutons d'action et compte à
  rebours.
- Module **Temps d'écran** :
  - temps au premier plan, avec détection d'absence et prise en compte du
    plein écran, et temps d'ouverture de chaque application ;
  - pages Journée, Historique, Applications, fiche d'application, Budgets et
    Réglages du suivi ;
  - six widgets ;
  - budgets quotidiens qui préviennent ou ferment l'application, avec
    rallonge depuis la notification.
- Thème clair et sombre selon Windows, police Geist embarquée.
- Installateur par utilisateur (Inno Setup), archive portable, et
  installation depuis le programme lui-même.
- Intégration continue et publication automatique sur GitHub Actions.

[Non publié]: https://github.com/pedrokarim/atelier/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/pedrokarim/atelier/releases/tag/v0.1.0
