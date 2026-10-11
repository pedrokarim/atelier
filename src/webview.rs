//! Fenêtres Win32 qui hébergent une vue web.
//!
//! Trois genres de fenêtres partagent ce code : la fenêtre principale, les
//! widgets épinglés sur le bureau et la notification. Les deux dernières sont
//! « sans cadre » : le cadre système est retiré par `WM_NCCALCSIZE`, mais la
//! fenêtre garde l'ombre et les coins arrondis que Windows 11 dessine autour
//! des fenêtres redimensionnables.
//!
//! La fenêtre ne se détruit jamais d'elle-même : à la fermeture, elle prévient
//! l'hôte, qui libère la vue web puis la fenêtre, dans cet ordre.

use std::num::NonZeroIsize;
use std::sync::mpsc::Sender;

use anyhow::{Context, Result, anyhow};
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{Rect, WebContext, WebView, WebViewBuilder};

use crate::ipc::{self, WindowId};
use crate::logging::log_line;
use crate::to_wide;

/// Genre de fenêtre : détermine le style, la classe et le comportement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Main,
    Widget,
    Notice,
}

impl Kind {
    fn class(self) -> PCWSTR {
        match self {
            Kind::Main => windows::core::w!("AtelierMain"),
            Kind::Widget => windows::core::w!("AtelierWidget"),
            Kind::Notice => windows::core::w!("AtelierNotice"),
        }
    }

    fn frameless(self) -> bool {
        self != Kind::Main
    }
}

pub enum Placement {
    Center,
    /// Coin inférieur droit de la zone de travail, avec une marge.
    BottomRight,
    /// Position en pixels physiques, telle qu'enregistrée.
    At(i32, i32),
}

pub struct Spec {
    pub kind: Kind,
    pub title: String,
    /// Taille logique (pixels à 96 ppp).
    pub width: i32,
    pub height: i32,
    pub min_width: i32,
    pub min_height: i32,
    pub placement: Placement,
    pub topmost: bool,
    pub html: String,
    pub dark: bool,
}

/// Données rattachées à la fenêtre, lues par la procédure de fenêtre.
struct Attached {
    webview: Option<WebView>,
    kind: Kind,
    dpi_scale: f64,
    min_width: i32,
    min_height: i32,
}

pub struct WebWindow {
    pub id: WindowId,
    pub hwnd: HWND,
    pub kind: Kind,
    attached: Box<Attached>,
}

/// Enveloppe la fenêtre hôte pour que la vue web puisse s'y attacher.
struct Parent(HWND);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let raw = NonZeroIsize::new(self.0.0 as isize).ok_or(HandleError::Unavailable)?;
        // La fenêtre vit aussi longtemps que la vue web : l'emprunt est valide.
        Ok(
            unsafe {
                WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(raw)))
            },
        )
    }
}

fn instance() -> Result<HINSTANCE> {
    Ok(unsafe { GetModuleHandleW(None) }
        .context("module courant introuvable")?
        .into())
}

/// Icône embarquée par le script de compilation.
fn application_icon(instance: HINSTANCE) -> HICON {
    for id in [1usize, 0x101] {
        if let Ok(icon) = unsafe { LoadIconW(Some(instance), PCWSTR(id as *const u16)) } {
            return icon;
        }
    }
    HICON::default()
}

pub fn register_class(class: PCWSTR, procedure: WNDPROC) -> Result<()> {
    let instance = instance()?;

    unsafe {
        let mut existing = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            ..Default::default()
        };
        if GetClassInfoExW(Some(instance), class, &mut existing).is_ok() {
            return Ok(());
        }

        let definition = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: procedure,
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: class,
            hIcon: application_icon(instance),
            hIconSm: application_icon(instance),
            ..Default::default()
        };

        if RegisterClassExW(&definition) == 0 {
            return Err(anyhow!("enregistrement de la classe de fenêtre impossible"));
        }
    }

    Ok(())
}

/// Windows est-il en thème sombre pour les applications ?
pub fn prefers_dark() -> bool {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_value::<u32, _>("AppsUseLightTheme"))
        .map(|light| light == 0)
        .unwrap_or(false)
}

pub fn apply_dark_title_bar(hwnd: HWND, dark: bool) {
    let enabled = windows::core::BOOL(dark as i32);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &enabled as *const _ as *const _,
            size_of::<windows::core::BOOL>() as u32,
        );
    }
}

impl WebWindow {
    pub fn create(
        id: WindowId,
        spec: Spec,
        messages: Sender<(WindowId, String)>,
        context: &mut WebContext,
    ) -> Result<Self> {
        let instance = instance()?;
        register_class(spec.kind.class(), Some(window_proc))?;

        let mut attached = Box::new(Attached {
            webview: None,
            kind: spec.kind,
            dpi_scale: 1.0,
            min_width: spec.min_width,
            min_height: spec.min_height,
        });

        let (style, mut extended) = match spec.kind {
            Kind::Main => (
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                WINDOW_EX_STYLE::default(),
            ),
            // WS_THICKFRAME n'est là que pour l'ombre et les coins arrondis :
            // le cadre est retiré, et le redimensionnement refusé.
            Kind::Widget => (WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN, WS_EX_TOOLWINDOW),
            Kind::Notice => (
                WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN,
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            ),
        };
        if spec.topmost {
            extended |= WS_EX_TOPMOST;
        }

        let title = to_wide(&spec.title);
        let hwnd = unsafe {
            CreateWindowExW(
                extended,
                spec.kind.class(),
                PCWSTR(title.as_ptr()),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                spec.width,
                spec.height,
                None,
                None,
                Some(instance),
                Some(&mut *attached as *mut Attached as *mut _),
            )
        }
        .context("création de la fenêtre")?;

        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        attached.dpi_scale = f64::from(dpi) / 96.0;

        if spec.dark {
            apply_dark_title_bar(hwnd, true);
        }

        if spec.kind.frameless() {
            unsafe {
                let corner = DWMWCP_ROUND;
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    &corner as *const _ as *const _,
                    size_of_val(&corner) as u32,
                );
                let margins = MARGINS {
                    cxLeftWidth: 1,
                    cxRightWidth: 1,
                    cyTopHeight: 1,
                    cyBottomHeight: 1,
                };
                let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
            }
        }

        let mut window = WebWindow {
            id,
            hwnd,
            kind: spec.kind,
            attached,
        };
        window.place(spec.width, spec.height, &spec.placement);

        let background = if spec.dark {
            (26, 26, 25, 255)
        } else {
            (249, 249, 247, 255)
        };

        let builder = WebViewBuilder::new_with_web_context(context)
            .with_html(spec.html)
            .with_background_color(background)
            .with_devtools(cfg!(debug_assertions))
            .with_ipc_handler(move |request| {
                if messages.send((id, request.into_body())).is_ok() {
                    ipc::wake();
                }
            });

        let webview = builder.build_as_child(&Parent(hwnd)).context(
            "création de la vue web impossible : le runtime WebView2 est-il installé ? \
             Il accompagne Microsoft Edge",
        )?;

        window.attached.webview = Some(webview);
        window.resize_webview();

        Ok(window)
    }

    /// Donne sa taille logique à la fenêtre et la positionne.
    fn place(&mut self, width: i32, height: i32, placement: &Placement) {
        let scale = self.attached.dpi_scale;
        let mut outer = RECT {
            left: 0,
            top: 0,
            right: (f64::from(width) * scale) as i32,
            bottom: (f64::from(height) * scale) as i32,
        };

        unsafe {
            if self.kind == Kind::Main {
                let _ = AdjustWindowRectEx(
                    &mut outer,
                    WS_OVERLAPPEDWINDOW,
                    false,
                    WINDOW_EX_STYLE::default(),
                );
            }

            let outer_width = outer.right - outer.left;
            let outer_height = outer.bottom - outer.top;

            let mut work = RECT::default();
            let _ = SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some(&mut work as *mut RECT as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );

            let margin = (16.0 * scale) as i32;
            let (x, y) = match placement {
                Placement::Center => (
                    work.left + (work.right - work.left - outer_width) / 2,
                    work.top + (work.bottom - work.top - outer_height) / 2,
                ),
                Placement::BottomRight => (
                    work.right - outer_width - margin,
                    work.bottom - outer_height - margin,
                ),
                Placement::At(x, y) => (*x, *y),
            };

            let _ = SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                outer_width,
                outer_height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    /// Change la taille logique sans déplacer la fenêtre.
    pub fn resize(&mut self, width: i32, height: i32) {
        let rect = self.rect();
        self.place(width, height, &Placement::At(rect.left, rect.top));
        self.resize_webview();
    }

    pub fn rect(&self) -> RECT {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut rect);
        }
        rect
    }

    fn resize_webview(&self) {
        unsafe { resize_webview(self.hwnd, &self.attached) };
    }

    pub fn show(&self, activate: bool) {
        unsafe {
            let _ = ShowWindow(
                self.hwnd,
                if activate { SW_SHOW } else { SW_SHOWNOACTIVATE },
            );
            if activate {
                if IsIconic(self.hwnd).as_bool() {
                    let _ = ShowWindow(self.hwnd, SW_RESTORE);
                }
                let _ = SetForegroundWindow(self.hwnd);
                if let Some(webview) = &self.attached.webview {
                    let _ = webview.focus();
                }
            }
        }
    }

    pub fn set_visible(&self, visible: bool) {
        unsafe {
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }

    pub fn set_topmost(&self, topmost: bool) {
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(if topmost {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                }),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    /// Commence le déplacement de la fenêtre, comme si l'utilisateur avait
    /// saisi une barre de titre. Appelé pendant que le bouton est enfoncé.
    pub fn begin_drag(&self) {
        unsafe {
            let _ = ReleaseCapture();
            let _ = SendMessageW(
                self.hwnd,
                WM_NCLBUTTONDOWN,
                Some(WPARAM(HTCAPTION as usize)),
                Some(LPARAM(0)),
            );
        }
    }

    pub fn eval(&self, script: &str) {
        if let Some(webview) = &self.attached.webview
            && let Err(error) = webview.evaluate_script(script)
        {
            log_line!("script refusé par la vue web : {error}");
        }
    }

    pub fn set_dark(&self, dark: bool) {
        if self.kind == Kind::Main {
            apply_dark_title_bar(self.hwnd, dark);
        }
    }
}

impl Drop for WebWindow {
    fn drop(&mut self) {
        // La vue web doit disparaître avant la fenêtre qui l'héberge.
        self.attached.webview = None;
        unsafe {
            if IsWindow(Some(self.hwnd)).as_bool() {
                SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

unsafe fn resize_webview(hwnd: HWND, attached: &Attached) {
    let Some(webview) = &attached.webview else {
        return;
    };

    let mut client = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut client) }.is_err() {
        return;
    }

    let scale = attached.dpi_scale;
    let width = f64::from(client.right - client.left) / scale;
    let height = f64::from(client.bottom - client.top) / scale;

    let _ = webview.set_bounds(Rect {
        position: LogicalPosition::new(0.0, 0.0).into(),
        size: LogicalSize::new(width.max(1.0), height.max(1.0)).into(),
    });
}

unsafe fn attached_from<'a>(hwnd: HWND) -> Option<&'a mut Attached> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Attached;
    if pointer.is_null() {
        None
    } else {
        Some(unsafe { &mut *pointer })
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_NCCREATE => {
                let create = lparam.0 as *const CREATESTRUCTW;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            // Sans cadre : toute la fenêtre est zone cliente.
            WM_NCCALCSIZE
                if wparam.0 != 0 && attached_from(hwnd).is_some_and(|a| a.kind.frameless()) =>
            {
                LRESULT(0)
            }
            WM_NCHITTEST if attached_from(hwnd).is_some_and(|a| a.kind.frameless()) => {
                LRESULT(HTCLIENT as isize)
            }
            WM_MOUSEACTIVATE if attached_from(hwnd).is_some_and(|a| a.kind == Kind::Notice) => {
                LRESULT(MA_NOACTIVATE as isize)
            }
            WM_SIZE => {
                if let Some(attached) = attached_from(hwnd) {
                    resize_webview(hwnd, attached);
                }
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                if let Some(attached) = attached_from(hwnd) {
                    let info = lparam.0 as *mut MINMAXINFO;
                    if !info.is_null() && attached.min_width > 0 {
                        let scale = attached.dpi_scale;
                        (*info).ptMinTrackSize.x = (f64::from(attached.min_width) * scale) as i32;
                        (*info).ptMinTrackSize.y = (f64::from(attached.min_height) * scale) as i32;
                    }
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                if let Some(attached) = attached_from(hwnd) {
                    let dpi = (wparam.0 & 0xFFFF) as u32;
                    attached.dpi_scale = f64::from(dpi.max(96)) / 96.0;

                    let suggested = lparam.0 as *const RECT;
                    if !suggested.is_null() {
                        let _ = SetWindowPos(
                            hwnd,
                            None,
                            (*suggested).left,
                            (*suggested).top,
                            (*suggested).right - (*suggested).left,
                            (*suggested).bottom - (*suggested).top,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                    resize_webview(hwnd, attached);
                }
                LRESULT(0)
            }
            WM_EXITSIZEMOVE => {
                ipc::post_to_host(ipc::WM_APP_WINDOW_MOVED, hwnd.0 as isize);
                LRESULT(0)
            }
            WM_CLOSE => {
                ipc::post_to_host(ipc::WM_APP_WINDOW_CLOSED, hwnd.0 as isize);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
