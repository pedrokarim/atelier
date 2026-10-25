//! Ce que Windows dit de sa mémoire et de ses processus, et les deux actions
//! possibles sur un processus : demander à ses fenêtres de se fermer, ou
//! l'arrêter.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::io::Read;
use std::sync::{Mutex, OnceLock};

use anyhow::{Result, anyhow};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{
    GetPerformanceInfo, GetProcessMemoryInfo, PERFORMANCE_INFORMATION, PROCESS_MEMORY_COUNTERS,
    PROCESS_MEMORY_COUNTERS_EX2,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    GetProcessHandleCount, GetProcessTimes, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, QueryFullProcessImageNameW,
    TerminateProcess,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId,
    IsWindowVisible, PostMessageW, WM_CLOSE,
};
use windows::core::{BOOL, PWSTR};

use super::analysis::{Memory, Process};

/// Poignée de processus, refermée en fin de portée.
struct Opened(HANDLE);

impl Drop for Opened {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Option<Opened> {
    unsafe { OpenProcess(access, false, pid) }.ok().map(Opened)
}

pub fn memory() -> Memory {
    let mut info = PERFORMANCE_INFORMATION {
        cb: size_of::<PERFORMANCE_INFORMATION>() as u32,
        ..Default::default()
    };
    if unsafe { GetPerformanceInfo(&mut info, info.cb) }.is_err() {
        return Memory::default();
    }

    let pages = |count: usize| count as u64 * info.PageSize as u64;
    Memory {
        total: pages(info.PhysicalTotal),
        available: pages(info.PhysicalAvailable),
        commit: pages(info.CommitTotal),
        commit_limit: pages(info.CommitLimit),
        kernel_paged: pages(info.KernelPaged),
        kernel_nonpaged: pages(info.KernelNonpaged),
        cache: pages(info.SystemCache),
        handles: info.HandleCount,
        processes: info.ProcessCount,
        threads: info.ThreadCount,
    }
}

/// Tous les processus, avec ce qu'on peut en lire sans être administrateur.
/// Les processus protégés de Windows ne livrent que leur nom.
pub fn processes() -> Vec<Process> {
    let windowed = windowed_processes();

    entries()
        .into_iter()
        .filter(|process| process.pid != 0)
        .map(|mut process| {
            process.windowed = windowed.contains(&process.pid);
            let mut session = 0u32;
            if unsafe { ProcessIdToSessionId(process.pid, &mut session) }.is_ok() {
                process.session = Some(session);
            }
            if let Some(handle) = open(process.pid, PROCESS_QUERY_LIMITED_INFORMATION) {
                inspect(&mut process, handle.0);
            }
            process.console = process.path.as_deref().is_some_and(is_console);
            process
        })
        .collect()
}

/// Numéro, parent, nom et fils de chaque processus.
fn entries() -> Vec<Process> {
    let mut list = Vec::with_capacity(400);

    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return list;
        };

        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let length = entry
                    .szExeFile
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(entry.szExeFile.len());
                list.push(Process {
                    pid: entry.th32ProcessID,
                    parent: entry.th32ParentProcessID,
                    name: String::from_utf16_lossy(&entry.szExeFile[..length]),
                    threads: entry.cntThreads,
                    ..Default::default()
                });
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
    }

    list
}

fn inspect(process: &mut Process, handle: HANDLE) {
    unsafe {
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        if QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
        .is_ok()
        {
            process.path = Some(String::from_utf16_lossy(&buffer[..size as usize]));
        }

        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user).is_ok() {
            process.started = Some(unix_ms(creation));
            process.cpu_time = ticks(kernel) + ticks(user);
        }

        // La version étendue des compteurs donne la mémoire privée
        // réellement présente en mémoire physique, sans les pages partagées.
        let mut counters = PROCESS_MEMORY_COUNTERS_EX2 {
            cb: size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
            ..Default::default()
        };
        if GetProcessMemoryInfo(
            handle,
            &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX2 as *mut PROCESS_MEMORY_COUNTERS,
            counters.cb,
        )
        .is_ok()
        {
            process.private_bytes = counters.PrivateWorkingSetSize as u64;
            process.commit = counters.PrivateUsage as u64;
        }

        let mut handles = 0u32;
        if GetProcessHandleCount(handle, &mut handles).is_ok() {
            process.handles = handles;
        }
    }
}

/// Programme en ligne de commande (sous-système console), d'après l'en-tête
/// de son exécutable. Le résultat est gardé : un exécutable ne change pas de
/// nature d'un relevé à l'autre.
fn is_console(path: &str) -> bool {
    static KNOWN: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let known = KNOWN.get_or_init(Mutex::default);
    if let Some(console) = known.lock().unwrap().get(path) {
        return *console;
    }
    let console = read_subsystem(path) == Some(IMAGE_SUBSYSTEM_WINDOWS_CUI);
    known.lock().unwrap().insert(path.to_string(), console);
    console
}

const IMAGE_SUBSYSTEM_WINDOWS_CUI: u16 = 3;

/// Champ `Subsystem` de l'en-tête PE : même position en 32 et en 64 bits,
/// 92 octets après la signature « PE » et ses deux octets nuls.
fn read_subsystem(path: &str) -> Option<u16> {
    let mut header = [0u8; 1024];
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.read(&mut header).ok()?;
    let header = &header[..length];

    let at = |offset: usize, size: usize| header.get(offset..offset + size);
    if at(0, 2)? != b"MZ" {
        return None;
    }
    let pe = u32::from_le_bytes(at(0x3c, 4)?.try_into().ok()?) as usize;
    if at(pe, 4)? != b"PE\0\0" {
        return None;
    }
    Some(u16::from_le_bytes(at(pe + 92, 2)?.try_into().ok()?))
}

/// FILETIME : centaines de nanosecondes.
fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

/// Date FILETIME (depuis le 1er janvier 1601) en millisecondes Unix.
fn unix_ms(time: FILETIME) -> i64 {
    (ticks(time) / 10_000) as i64 - 11_644_473_600_000
}

fn start_ms(handle: HANDLE) -> Option<i64> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) }
        .ok()
        .map(|_| unix_ms(creation))
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

fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// Fenêtre principale qu'un utilisateur peut voir : visible, titrée, sans
/// propriétaire, et non masquée par DWM.
fn is_visible_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
            return false;
        }
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
            return false;
        }
        let mut cloaked = 0u32;
        let hidden = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0;
        !hidden
    }
}

fn windowed_processes() -> HashSet<u32> {
    top_level_windows()
        .into_iter()
        .filter(|hwnd| is_visible_window(*hwnd))
        .map(window_pid)
        .collect()
}

/// Demande poliment aux fenêtres d'un processus de se fermer : l'application
/// peut proposer d'enregistrer. Renvoie le nombre de fenêtres prévenues.
pub fn close_windows(pid: u32) -> usize {
    let mut sent = 0;
    for hwnd in top_level_windows() {
        if window_pid(hwnd) == pid && is_visible_window(hwnd) {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            sent += 1;
        }
    }
    sent
}

/// Arrête un processus, après avoir vérifié que c'est bien celui lancé à
/// l'heure `started` : son numéro a pu être réattribué entre-temps.
pub fn terminate(pid: u32, started: Option<i64>) -> Result<()> {
    let handle =
        open(pid, PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION).ok_or_else(|| {
            anyhow!("accès refusé\u{a0}: le processus est peut-être lancé en administrateur")
        })?;

    if started.is_some() && start_ms(handle.0) != started {
        return Err(anyhow!("ce processus s’est déjà terminé"));
    }

    unsafe { TerminateProcess(handle.0, 1) }
        .map_err(|error| anyhow!("arrêt refusé par Windows\u{a0}: {}", error.message()))
}
