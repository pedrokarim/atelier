//! Le contrat d'un module.
//!
//! Atelier ne sait rien faire par lui-même : il héberge des modules. Chacun
//! apporte deux moitiés.
//!
//! - **Côté Rust** (ce trait) : le travail de fond et les réponses aux
//!   requêtes de ses pages et de ses widgets.
//! - **Côté page** (`ui/modules/<id>.js`) : ses pages et ses widgets,
//!   déclarés par `Atelier.module({…})` et `Atelier.widget({…})`.
//!
//! Ajouter un module, par exemple un widget de bourse :
//!
//! 1. écrire `src/modules/markets.rs`, qui implémente `Module` ;
//! 2. écrire `ui/modules/markets.js`, qui déclare le widget ;
//! 3. l'inscrire dans `registry()` ci-dessous.
//!
//! Rien d'autre à toucher : la grille d'accueil, le catalogue des widgets,
//! l'épinglage sur le bureau et les notifications sont fournis par l'hôte.

use std::sync::Arc;

use serde_json::Value;

use crate::ipc::{HostSender, Responder};

pub mod screen_time;
pub mod system;

/// Ce que l'hôte confie à un module au démarrage.
#[derive(Clone)]
pub struct ModuleContext {
    pub host: HostSender,
}

pub trait Module: Send + Sync {
    /// Identifiant stable : préfixe de ses widgets, nom de son dossier de
    /// données, cible des requêtes de ses pages.
    fn id(&self) -> &'static str;

    /// Script de la moitié « page », embarqué dans le binaire.
    fn script(&self) -> &'static str;

    /// Widgets posés sur l'accueil au tout premier lancement : (widget, taille).
    fn default_widgets(&self) -> Vec<(&'static str, &'static str)> {
        Vec::new()
    }

    /// Démarre le travail de fond, s'il y en a.
    fn start(&self, _context: ModuleContext) {}

    /// Répond à une requête `request(method, params)` venue d'une page ou d'un
    /// widget du module.
    fn handle(&self, method: &str, params: Value, reply: Responder);

    /// Un bouton d'une notification du module a été actionné.
    fn notice_action(&self, _notice: &str, _action: &str) {}

    /// Ligne ajoutée à l'infobulle de l'icône de notification.
    fn tray_status(&self) -> Option<String> {
        None
    }

    /// Enregistrement immédiat : fermeture de session, mise en veille. Le
    /// système peut couper le programme juste après.
    fn flush(&self) {}

    /// Arrêt du programme : dernière occasion d'enregistrer.
    fn stop(&self) {}
}

/// Les modules de l'application, dans l'ordre de la barre latérale.
pub fn registry() -> Vec<Arc<dyn Module>> {
    vec![
        Arc::new(screen_time::ScreenTime::new()),
        Arc::new(system::System::new()),
    ]
}
