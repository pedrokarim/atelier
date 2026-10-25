# Journal des modifications

Toutes les évolutions notables d'Atelier sont consignées ici. Le format suit
[Keep a Changelog](https://keepachangelog.com/fr/1.1.0/), et les versions
respectent le [versionnage sémantique](https://semver.org/lang/fr/).

## [Non publié]

## [0.2.1] - 2026-10-25

### Modifié

- Nouveau logo : les vraies tailles de widgets d'Atelier sur leur grille, et
  une case libre marquée d'un « + », la place du prochain widget. Il
  remplace l'icône de l'exécutable, de l'installateur, de la zone de
  notification et de la barre latérale.
- L'icône de la zone de notification est dessinée au pixel près pour chaque
  échelle d'affichage (16, 20, 24 et 32 px) : Windows reçoit la taille
  qu'il affiche, au lieu de réduire une grande image.
- Le logo et les icônes sont construits par `scripts/logo/build_logo.py`, qui
  remplace `scripts/make-icon.ps1`.

### Corrigé

- Une notification dont le texte passait à la ligne après le chargement de la
  police coupait ses boutons : la fenêtre suit désormais la hauteur de son
  contenu à chaque changement.
- Les notifications du module Système portent leur propre pictogramme
  (mémoire, alerte) au lieu du sablier des budgets.

## [0.2.0] - 2026-10-25

### Ajouté

- Module **Système** : ce qui encombre l'ordinateur, et de quoi y remédier.
  - Répartition de la mémoire (applications, noyau, partagé, disponible),
    avec des alertes quand le noyau gonfle, quand la mémoire disponible
    manque ou quand la mémoire engagée approche de sa limite.
  - Processus anormaux : handles accumulés, mémoire accaparée, processeur
    sollicité sans fenêtre, commande restée seule après la fermeture de son
    terminal.
  - Actions : demander la fermeture, arrêter un processus ou une
    application entière, ne plus signaler une application. Les processus de
    Windows et d'Atelier sont protégés.
  - Notification quand un processus devient anormal, avec un bouton pour
    l'arrêter.
  - Pages *Encombrement* et *Processus*, widgets *Mémoire*, *Ce qui encombre*
    et *Les plus gourmandes*.
- Audit de mise en page : les pages et widgets du module Système, sur un
  relevé fictif.

## [0.1.2] - 2026-10-12

### Corrigé

- Le graphique du widget « Temps d'écran » recouvrait la variation par
  rapport à la moyenne : dans les widgets, les graphiques occupent
  désormais la hauteur qui reste dans la carte, sans la dépasser.
- La ligne de moyenne des graphiques passe au-dessus des barres et se lit.
- Le texte des notifications débordait sur leurs boutons : la fenêtre prend
  désormais la hauteur de son contenu, les boutons restent groupés, et un
  texte trop long est coupé proprement avec le texte complet au survol.
- Le widget « Budgets du jour » coupait les noms longs, repoussait les
  durées hors de la carte et tronquait sa dernière ligne ; les listes des
  widgets s'arrêtent désormais à ce qui tient.
- Le widget « En ce moment » débordait de sa carte en petite taille.
- Une colonne ou une carte ne s'élargit plus au-delà de la fenêtre pour un
  nom très long : les noms tronqués ont leur nom complet en infobulle.
- Le tableau des applications ne déborde plus dans une fenêtre étroite : la
  colonne « Ouvert (total) » s'efface sous 1 000 pixels de large.
- Une copie portable d'Atelier peut tourner à côté de la version installée :
  il y a une instance par dossier de données, et non plus une seule en tout.

### Ajouté

- Audit de mise en page (`scripts/audit/audit.py`) : chaque écran, dans les
  deux thèmes, à cinq largeurs et avec trois jeux de données, mesuré dans la
  page et capturé.
- Comparaison visuelle entre deux versions (`scripts/audit/compare.py`) :
  chaque écran avant et après, avec les zones qui ont changé.
- Laboratoire des widgets (route `lab`) : chaque widget dans chacune de ses
  tailles.

## [0.1.1] - 2026-10-11

### Corrigé

- Les options de ligne de commande (`--data-dir`, `--help`…) ne plantent
  plus quand leur sortie est capturée ou redirigée.
- Compatibilité avec Clippy 1.99.

### Modifié

- Dépendances à jour : `dirs` 7.0.0, `tray-icon` 0.24.2, `base64` 0.23.1.
- Les notes de release ne reprennent plus les liens de bas de page du
  journal.

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

[Non publié]: https://github.com/pedrokarim/atelier/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/pedrokarim/atelier/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/pedrokarim/atelier/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/pedrokarim/atelier/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/pedrokarim/atelier/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/pedrokarim/atelier/releases/tag/v0.1.0
