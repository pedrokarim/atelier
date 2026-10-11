//! Installation locale, sans privilèges administrateur.
//!
//! Le binaire est autonome : « installer » se réduit à le poser à un endroit
//! stable et à le rendre trouvable dans le menu Démarrer.
//!
//! ```text
//! %LOCALAPPDATA%\Programs\atelier\atelier.exe   copie stable
//! …\Menu Démarrer\Programmes\Atelier.lnk        recherche Windows
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IPersistFile,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{Interface, PCWSTR};

use crate::logging::log_line;
use crate::to_wide;

const FOLDER_NAME: &str = "atelier";
const EXECUTABLE_NAME: &str = "atelier.exe";
const SHORTCUT_NAME: &str = "Atelier.lnk";

/// Suffixe donné à l'ancien binaire pendant une mise à jour : Windows autorise
/// le *renommage* d'un exécutable en cours d'exécution, jamais son écrasement.
const REPLACED_SUFFIX: &str = ".ancien";

pub fn install_dir() -> Result<PathBuf> {
    let base =
        dirs::data_local_dir().ok_or_else(|| anyhow!("dossier des données locales introuvable"))?;
    Ok(base.join("Programs").join(FOLDER_NAME))
}

pub fn installed_executable() -> Result<PathBuf> {
    Ok(install_dir()?.join(EXECUTABLE_NAME))
}

fn shortcut_path() -> Result<PathBuf> {
    let base = dirs::data_dir().ok_or_else(|| anyhow!("dossier Roaming introuvable"))?;
    Ok(base
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join(SHORTCUT_NAME))
}

/// État de l'installation, tel qu'affiché par le tableau de bord.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    pub current_exe: PathBuf,
    pub installed_exe: PathBuf,
    pub installed: bool,
    pub running_from_install: bool,
    pub has_shortcut: bool,
}

pub fn status() -> Result<Status> {
    let current_exe = std::env::current_exe().context("chemin de l'exécutable introuvable")?;
    let installed_exe = installed_executable()?;

    // Comparaison canonique : les chemins peuvent différer par la casse.
    let same = std::fs::canonicalize(&current_exe)
        .ok()
        .zip(std::fs::canonicalize(&installed_exe).ok())
        .is_some_and(|(a, b)| a == b);

    Ok(Status {
        installed: installed_exe.is_file(),
        running_from_install: same,
        has_shortcut: shortcut_path().is_ok_and(|path| path.is_file()),
        current_exe,
        installed_exe,
    })
}

/// Exécutable à inscrire au démarrage : la copie installée si elle existe,
/// sinon le binaire courant.
pub fn preferred_executable() -> Result<PathBuf> {
    let installed = installed_executable()?;
    if installed.is_file() {
        return Ok(installed);
    }
    std::env::current_exe().context("chemin de l'exécutable introuvable")
}

/// Supprime le binaire laissé par une mise à jour précédente.
pub fn cleanup_previous() {
    let Ok(dir) = install_dir() else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.to_string_lossy().ends_with(REPLACED_SUFFIX) && std::fs::remove_file(&path).is_ok()
        {
            log_line!("ancien binaire supprimé : {}", path.display());
        }
    }
}

fn create_shortcut(target: &Path, link: &Path) -> Result<()> {
    let target_wide = to_wide(&target.to_string_lossy());
    let link_wide = to_wide(&link.to_string_lossy());
    let description = to_wide("Atelier : widgets et outils du bureau");

    unsafe {
        // COM peut déjà être initialisé dans un autre mode par une dépendance,
        // ce qui n'empêche pas d'obtenir l'objet ci-dessous.
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let result = (|| -> Result<()> {
            let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .context("création de l'objet raccourci")?;

            shell_link.SetPath(PCWSTR(target_wide.as_ptr()))?;
            shell_link.SetDescription(PCWSTR(description.as_ptr()))?;
            shell_link.SetIconLocation(PCWSTR(target_wide.as_ptr()), 0)?;

            let persist: IPersistFile = shell_link.cast()?;
            persist
                .Save(PCWSTR(link_wide.as_ptr()), true)
                .context("enregistrement du raccourci")?;

            Ok(())
        })();

        if initialized.is_ok() {
            CoUninitialize();
        }

        result
    }
}

/// Installe le binaire courant et crée le raccourci du menu Démarrer.
pub fn install() -> Result<PathBuf> {
    let status = status()?;
    let destination = status.installed_exe.clone();

    if !status.running_from_install {
        let directory = install_dir()?;
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("création de {}", directory.display()))?;

        if destination.exists() {
            let retired = destination.with_extension(format!("exe{REPLACED_SUFFIX}"));
            let _ = std::fs::remove_file(&retired);
            std::fs::rename(&destination, &retired).with_context(|| {
                format!(
                    "remplacement impossible : {} est peut-être en cours d'exécution",
                    destination.display()
                )
            })?;
        }

        std::fs::copy(&status.current_exe, &destination).with_context(|| {
            format!(
                "copie de {} vers {}",
                status.current_exe.display(),
                destination.display()
            )
        })?;
    }

    let link = shortcut_path()?;
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    create_shortcut(&destination, &link)?;

    // L'entrée de démarrage doit désormais viser la copie installée.
    if crate::autostart::is_enabled() {
        crate::autostart::enable()?;
    }

    log_line!("installé dans {}", destination.display());
    Ok(destination)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removal {
    NothingToDo,
    Removed,
    /// Le processus courant s'exécute depuis le dossier : il doit d'abord se
    /// terminer.
    Deferred,
}

/// Retire le raccourci, l'entrée de démarrage et le dossier d'installation.
///
/// Les données d'utilisation sont conservées : elles représentent l'historique
/// de l'utilisateur.
pub fn uninstall() -> Result<Removal> {
    // Installé par l'installateur : c'est son désinstalleur qui fait foi,
    // sans quoi l'entrée « Applications installées » de Windows resterait
    // orpheline. Il arrête lui-même le programme.
    let uninstaller = install_dir()?.join("unins000.exe");
    if uninstaller.is_file() {
        Command::new(&uninstaller)
            .spawn()
            .context("lancement du désinstalleur")?;
        return Ok(Removal::Deferred);
    }

    match std::fs::remove_file(shortcut_path()?) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("suppression du raccourci"),
    }
    crate::autostart::disable()?;

    let status = status()?;
    let directory = install_dir()?;

    if !directory.exists() {
        return Ok(Removal::NothingToDo);
    }

    if status.running_from_install {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        // Un binaire ne peut pas se supprimer lui-même : un interpréteur de
        // commandes patiente, puis efface. `ping` sert de temporisation car
        // `timeout` échoue sans console.
        Command::new("cmd")
            .arg("/C")
            .arg(format!(
                "ping 127.0.0.1 -n 4 >nul & rmdir /s /q \"{}\"",
                directory.display()
            ))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("planification de la suppression")?;
        return Ok(Removal::Deferred);
    }

    std::fs::remove_dir_all(&directory)
        .with_context(|| format!("suppression de {}", directory.display()))?;
    Ok(Removal::Removed)
}
