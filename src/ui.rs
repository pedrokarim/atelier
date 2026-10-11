//! Assemblage des pages embarquées.
//!
//! Chaque page est un gabarit (`ui/*.html`) dans lequel l'hôte insère la
//! feuille de style commune, le noyau JavaScript, les scripts des modules et
//! les données de démarrage. Tout est embarqué dans le binaire.

use std::sync::Arc;

use serde_json::Value;

use crate::modules::Module;

const CORE_CSS: &str = include_str!("../ui/core.css");
const CORE_JS: &str = include_str!("../ui/core.js");
const SHELL: &str = include_str!("../ui/shell.html");
const WIDGET: &str = include_str!("../ui/widget.html");
const NOTICE: &str = include_str!("../ui/notice.html");

/// Geist, de Vercel, sous licence SIL OFL 1.1 (`assets/fonts/OFL.txt`).
/// Embarquée pour que l'interface ait la même allure hors ligne et sur toute
/// machine.
const GEIST_SANS: &[u8] = include_bytes!("../assets/fonts/Geist-Variable.woff2");
const GEIST_MONO: &[u8] = include_bytes!("../assets/fonts/GeistMono-Variable.woff2");

/// Déclarations `@font-face`, polices incluses en données : la page est
/// chargée depuis une chaîne et n'a pas d'adresse d'où charger un fichier.
fn font_faces() -> &'static str {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use std::sync::OnceLock;

    static FACES: OnceLock<String> = OnceLock::new();
    FACES.get_or_init(|| {
        let face = |family: &str, bytes: &[u8]| {
            format!(
                "@font-face {{ font-family: \"{family}\"; font-weight: 100 900; font-style: normal; \
                 font-display: block; src: url(data:font/woff2;base64,{}) format(\"woff2\"); }}\n",
                STANDARD.encode(bytes)
            )
        };
        face("Geist", GEIST_SANS) + &face("Geist Mono", GEIST_MONO)
    })
}

#[derive(Clone, Copy)]
pub enum Page {
    /// Fenêtre principale : accueil, pages des modules, réglages.
    Shell,
    /// Un widget épinglé sur le bureau.
    Widget,
    /// Notification dans le coin de l'écran.
    Notice,
}

/// Isole le script d'un module : une erreur dans l'un ne doit pas priver la
/// page des autres.
fn wrap_module(id: &str, script: &str) -> String {
    format!(
        "(function () {{\ntry {{\n{script}\n}} catch (error) {{ Atelier.moduleFailed({}, error); }}\n}})();\n",
        crate::ipc::json(id)
    )
}

pub fn render(page: Page, modules: &[Arc<dyn Module>], dark: bool, boot: &Value) -> String {
    let template = match page {
        Page::Shell => SHELL,
        Page::Widget => WIDGET,
        Page::Notice => NOTICE,
    };

    let scripts: String = match page {
        Page::Notice => String::new(),
        _ => modules
            .iter()
            .map(|module| wrap_module(module.id(), module.script()))
            .collect(),
    };

    let html = template
        .replacen("/*@core.css*/", &format!("{}{CORE_CSS}", font_faces()), 1)
        .replacen("/*@boot*/", &format!("window.ATELIER_BOOT = {};", boot), 1)
        .replacen("/*@core.js*/", CORE_JS, 1)
        .replacen("/*@modules*/", &scripts, 1);

    // Le thème est posé par l'hôte plutôt que laissé à la seule requête de
    // média : WebView2 ne suit pas toujours le réglage de Windows.
    if dark {
        html.replacen(
            "<html lang=\"fr\">",
            "<html lang=\"fr\" data-theme=\"dark\">",
            1,
        )
    } else {
        html.replacen(
            "<html lang=\"fr\">",
            "<html lang=\"fr\" data-theme=\"light\">",
            1,
        )
    }
}
