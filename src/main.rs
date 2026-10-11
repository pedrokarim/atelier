//! Atelier : application de bureau modulaire, à widgets.
//!
//! Le programme vit dans la zone de notification. Il héberge des modules
//! (Temps d'écran pour commencer), qui apportent des pages et des widgets.
//! Les widgets se posent sur l'accueil ou s'épinglent sur le bureau.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod install;
mod ipc;
mod layout;
mod logging;
mod modules;
mod paths;
mod shell;
mod ui;
mod webview;

use anyhow::{Result, anyhow};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, FindWindowW, PostMessageW,
};
use windows::core::w;

use crate::logging::log_line;

pub fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn main() {
    // Affiche les messages quand le binaire est lancé depuis un terminal,
    // alors qu'il est compilé en application fenêtrée.
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }

    logging::rotate_if_needed();
    install::cleanup_previous();

    if let Err(error) = run() {
        let message = format!("{error:#}");
        log_line!("erreur fatale : {message}");
        app::show_error(&message);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        None | Some("--show" | "--open") => run_resident(),
        Some("--help" | "-h" | "/?") => {
            print_help();
            Ok(())
        }
        Some("--install") => {
            let path = install::install()?;
            println!("Installé : {}", path.display());
            Ok(())
        }
        Some("--uninstall") => {
            match install::uninstall()? {
                install::Removal::NothingToDo => println!("Rien à désinstaller."),
                install::Removal::Removed => println!("Désinstallation terminée."),
                install::Removal::Deferred => println!("Désinstallation planifiée."),
            }
            Ok(())
        }
        Some("--data-dir") => {
            println!("{}", paths::data_dir()?.display());
            Ok(())
        }
        Some(other) => Err(anyhow!(
            "option inconnue : {other}\nUtilisez --help pour la liste des options."
        )),
    }
}

fn print_help() {
    println!(
        r#"Atelier : widgets et outils du bureau

Sans argument, le programme s'installe dans la zone de notification. S'il
tourne déjà, sa fenêtre principale s'ouvre.

Options :
  --show        Ouvre la fenêtre principale dès le démarrage
  --open <page> Ouvre directement une page (« home », « settings »,
                « screen-time/budgets »…)
  --install     Installe le programme et l'ajoute au menu Démarrer
  --uninstall   Retire l'installation, le raccourci et le démarrage
  --data-dir    Affiche le dossier des données
  --help        Affiche cette aide"#
    );
}

/// Une seule instance à la fois : la seconde se contente de réveiller la
/// première, qui ouvre sa fenêtre.
fn run_resident() -> Result<()> {
    let _guard = unsafe { CreateMutexW(None, true, w!("Local\\atelier-resident")) };

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        if let Ok(hwnd) = unsafe { FindWindowW(app::HOST_CLASS, None) } {
            unsafe {
                // Autorise l'instance résidente à passer au premier plan.
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                let _ = PostMessageW(
                    Some(hwnd),
                    ipc::WM_APP_SHOW,
                    Default::default(),
                    Default::default(),
                );
            }
        }
        return Ok(());
    }

    app::App::new()?.run()
}
