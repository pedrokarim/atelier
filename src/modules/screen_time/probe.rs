//! Ce que Windows sait des applications : fenêtre au premier plan, processus
//! lancés, présence de l'utilisateur, métadonnées et icônes des exécutables.
//!
//! Toutes ces fonctions sont des lectures, à l'exception de la fermeture
//! d'application qu'exige un budget épuisé.

use std::collections::HashSet;
use std::ffi::c_void;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetMonitorInfoW, GetObjectW, HBITMAP, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromWindow,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAGS_AND_ATTRIBUTES, GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::Shell::{
    SHDefExtractIconW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, EnumChildWindows, EnumWindows, GW_OWNER, GWL_EXSTYLE, GetClassNameW,
    GetForegroundWindow, GetIconInfo, GetWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowThreadProcessId, HICON, ICONINFO, IsWindowVisible, PostMessageW,
    WM_CLOSE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PCWSTR, PWSTR};

use crate::to_wide;

/// Exécutables de l'interface de Windows elle-même : menu Démarrer, recherche,
/// écran de verrouillage. Leur temps n'appartient à aucune application.
pub const SHELL_EXECUTABLES: &[&str] = &[
    "lockapp.exe",
    "shellexperiencehost.exe",
    "shellhost.exe",
    "searchhost.exe",
    "searchapp.exe",
    "startmenuexperiencehost.exe",
    "textinputhost.exe",
    "applicationframehost.exe",
    "dwm.exe",
    "rundll32.exe",
];

/// Applications toujours lancées, parce qu'elles sont le bureau lui-même :
/// seul leur temps au premier plan a un sens, jamais leur temps « ouvert ».
const ALWAYS_RUNNING: &[&str] = &[r"c:\windows\explorer.exe"];

pub fn is_always_running(key: &str) -> bool {
    ALWAYS_RUNNING.contains(&key)
}

pub fn is_shell_executable(path: &str) -> bool {
    let file = path.rsplit('\\').next().unwrap_or(path).to_lowercase();
    SHELL_EXECUTABLES.contains(&file.as_str())
}

/// Secondes écoulées depuis la dernière saisie au clavier ou à la souris.
pub fn idle_seconds() -> u32 {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };

    unsafe {
        if !GetLastInputInfo(&mut info).as_bool() {
            return 0;
        }
        GetTickCount().wrapping_sub(info.dwTime) / 1000
    }
}

fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

/// Les applications du Store sont hébergées par `ApplicationFrameHost.exe` :
/// le vrai processus est celui d'une fenêtre enfant.
fn hosted_pid(frame: HWND, host_pid: u32) -> u32 {
    struct Search {
        host_pid: u32,
        found: u32,
    }

    unsafe extern "system" fn visit(child: HWND, lparam: LPARAM) -> BOOL {
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let pid = window_pid(child);
        if pid != 0 && pid != search.host_pid {
            search.found = pid;
            return BOOL(0);
        }
        BOOL(1)
    }

    let mut search = Search { host_pid, found: 0 };
    unsafe {
        let _ = EnumChildWindows(
            Some(frame),
            Some(visit),
            LPARAM(&mut search as *mut Search as isize),
        );
    }

    if search.found != 0 {
        search.found
    } else {
        host_pid
    }
}

/// Processus réellement responsable d'une fenêtre.
fn effective_pid(hwnd: HWND) -> u32 {
    let pid = window_pid(hwnd);
    if class_name(hwnd) == "ApplicationFrameWindow" {
        hosted_pid(hwnd, pid)
    } else {
        pid
    }
}

/// La fenêtre couvre-t-elle tout son écran ? (jeu, vidéo, présentation)
fn is_fullscreen(hwnd: HWND) -> bool {
    // Le bureau lui-même couvre l'écran sans être une application.
    if matches!(class_name(hwnd).as_str(), "Progman" | "WorkerW") {
        return false;
    }

    unsafe {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }

        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }

        let screen = info.rcMonitor;
        rect.left <= screen.left
            && rect.top <= screen.top
            && rect.right >= screen.right
            && rect.bottom >= screen.bottom
    }
}

pub struct Foreground {
    pub pid: u32,
    pub fullscreen: bool,
}

pub fn foreground() -> Option<Foreground> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }

    let pid = effective_pid(hwnd);
    (pid != 0).then(|| Foreground {
        pid,
        fullscreen: is_fullscreen(hwnd),
    })
}

fn top_level_windows() -> Vec<HWND> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        list.push(hwnd);
        BOOL(1)
    }

    let mut list: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut list as *mut Vec<HWND> as isize));
    }
    list
}

/// Fenêtre qui aurait un bouton dans la barre des tâches.
fn is_app_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }

        if GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
            return false;
        }

        let extended = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if extended & WS_EX_TOOLWINDOW.0 != 0 && extended & WS_EX_APPWINDOW.0 == 0 {
            return false;
        }

        // Les fenêtres « masquées » par DWM (applications du Store en
        // suspens, bureaux virtuels) sont visibles pour Windows, pas pour
        // l'utilisateur.
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
        {
            return false;
        }

        GetWindowTextLengthW(hwnd) > 0
    }
}

/// Processus qui possèdent au moins une fenêtre d'application visible.
pub fn processes_with_windows() -> HashSet<u32> {
    top_level_windows()
        .into_iter()
        .filter(|hwnd| is_app_window(*hwnd))
        .map(effective_pid)
        .filter(|pid| *pid != 0)
        .collect()
}

/// Identifiants de tous les processus en cours.
pub fn running_processes() -> Vec<u32> {
    let mut pids = Vec::with_capacity(400);

    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return pids;
        };

        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                pids.push(entry.th32ProcessID);
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
    }

    pids
}

/// Chemin complet de l'exécutable d'un processus.
pub fn process_path(pid: u32) -> Option<String> {
    if pid == 0 || pid == 4 {
        return None;
    }

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        result.ok()?;
        Some(String::from_utf16_lossy(&buffer[..size as usize]))
    }
}

/// Heure de lancement d'un processus, en millisecondes depuis l'époque Unix.
pub fn process_start_ms(pid: u32) -> Option<i64> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        let _ = CloseHandle(handle);
        result.ok()?;

        // FILETIME : centaines de nanosecondes depuis le 1er janvier 1601.
        let ticks = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        Some((ticks / 10_000) as i64 - 11_644_473_600_000)
    }
}

/// Demande poliment aux fenêtres des processus donnés de se fermer.
pub fn close_windows(pids: &HashSet<u32>) -> usize {
    let mut sent = 0;
    for hwnd in top_level_windows() {
        if unsafe { IsWindowVisible(hwnd) }.as_bool() && pids.contains(&effective_pid(hwnd)) {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            sent += 1;
        }
    }
    sent
}

/// Termine un processus. Dernier recours, après `close_windows`.
pub fn terminate(pid: u32) -> bool {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) else {
            return false;
        };
        let done = TerminateProcess(handle, 1).is_ok();
        let _ = CloseHandle(handle);
        done
    }
}

// --- Métadonnées ---------------------------------------------------------

/// Nom lisible d'un exécutable : sa description, à défaut son nom de produit.
pub fn executable_name(path: &str) -> Option<String> {
    let wide = to_wide(path);

    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }

        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(
            PCWSTR(wide.as_ptr()),
            None,
            size,
            data.as_mut_ptr() as *mut c_void,
        )
        .ok()?;

        let mut translations: Vec<String> = Vec::new();
        let mut pointer: *mut c_void = std::ptr::null_mut();
        let mut length = 0u32;
        let query = to_wide(r"\VarFileInfo\Translation");
        if VerQueryValueW(
            data.as_ptr() as *const c_void,
            PCWSTR(query.as_ptr()),
            &mut pointer,
            &mut length,
        )
        .as_bool()
            && !pointer.is_null()
        {
            let pairs = std::slice::from_raw_parts(pointer as *const u16, (length / 2) as usize);
            for pair in pairs.chunks_exact(2) {
                translations.push(format!("{:04x}{:04x}", pair[0], pair[1]));
            }
        }
        translations.extend(["040904b0".to_string(), "040904e4".to_string()]);

        for field in ["FileDescription", "ProductName"] {
            for translation in &translations {
                let query = to_wide(&format!(r"\StringFileInfo\{translation}\{field}"));
                let mut pointer: *mut c_void = std::ptr::null_mut();
                let mut length = 0u32;
                if VerQueryValueW(
                    data.as_ptr() as *const c_void,
                    PCWSTR(query.as_ptr()),
                    &mut pointer,
                    &mut length,
                )
                .as_bool()
                    && !pointer.is_null()
                    && length > 1
                {
                    let text =
                        std::slice::from_raw_parts(pointer as *const u16, length as usize - 1);
                    let value = String::from_utf16_lossy(text).trim().to_string();
                    if !value.is_empty() {
                        return Some(value);
                    }
                }
            }
        }
    }

    None
}

/// Icône d'un exécutable, en URL de données PNG.
pub fn executable_icon(path: &str) -> Option<String> {
    let icon = extract_icon(path)?;
    let pixels = unsafe { icon_pixels(icon) };
    unsafe {
        let _ = DestroyIcon(icon);
    }

    let (width, height, rgba) = pixels?;
    let image = image::RgbaImage::from_raw(width, height, rgba)?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;

    Some(format!("data:image/png;base64,{}", STANDARD.encode(png)))
}

fn extract_icon(path: &str) -> Option<HICON> {
    let wide = to_wide(path);

    unsafe {
        // 64 px : net à l'affichage en 32 px sur un écran à forte densité.
        let mut large = HICON::default();
        if SHDefExtractIconW(PCWSTR(wide.as_ptr()), 0, 0, Some(&mut large), None, 64).is_ok()
            && !large.is_invalid()
        {
            return Some(large);
        }

        // Repli : l'icône que l'explorateur afficherait, en 32 px.
        let mut info = SHFILEINFOW::default();
        let found = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        (found != 0 && !info.hIcon.is_invalid()).then_some(info.hIcon)
    }
}

/// Lit les pixels d'une icône en RGBA.
unsafe fn icon_pixels(icon: HICON) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(icon, &mut info).ok()?;

        let result = (|| {
            if info.hbmColor.is_invalid() {
                return None;
            }

            let mut bitmap = BITMAP::default();
            if GetObjectW(
                info.hbmColor.into(),
                size_of::<BITMAP>() as i32,
                Some(&mut bitmap as *mut BITMAP as *mut c_void),
            ) == 0
            {
                return None;
            }

            let width = bitmap.bmWidth;
            let height = bitmap.bmHeight;
            if width <= 0 || height <= 0 {
                return None;
            }

            let mut color = read_bitmap(info.hbmColor, width, height)?;

            // Icône ancienne sans canal alpha : la transparence vient du masque.
            if color.chunks_exact(4).all(|pixel| pixel[3] == 0) {
                let mask = read_bitmap(info.hbmMask, width, height)?;
                for (pixel, mask_pixel) in color.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
                    pixel[3] = if mask_pixel[0] == 0 { 255 } else { 0 };
                }
            }

            // BGRA vers RGBA.
            for pixel in color.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }

            Some((width as u32, height as u32, color))
        })();

        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(info.hbmColor.into());
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(info.hbmMask.into());
        }

        result
    }
}

unsafe fn read_bitmap(bitmap: HBITMAP, width: i32, height: i32) -> Option<Vec<u8>> {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Hauteur négative : lignes de haut en bas.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr() as *mut c_void),
            &mut header,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);

        (lines > 0).then_some(pixels)
    }
}
