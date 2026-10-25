//! L'hôte : une boucle de messages, une icône de notification, des fenêtres
//! web, et l'aiguillage des requêtes vers les modules.
//!
//! Tout ce qui touche aux fenêtres se passe sur le fil principal. Les modules
//! travaillent où ils veulent et communiquent par `HostEvent`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};
use wry::WebContext;

use crate::ipc::{self, HostEvent, HostSender, Notice, Responder, WindowId};
use crate::layout::{self, Instance, Layout, Pinned};
use crate::logging::log_line;
use crate::modules::{Module, ModuleContext};
use crate::ui::{self, Page};
use crate::webview::{self, Kind, Placement, Spec, WebWindow};
use crate::{autostart, install, paths};

pub const HOST_CLASS: PCWSTR = w!("AtelierHost");

const TIMER_ID: usize = 1;

/// Largeur logique de la notification ; sa hauteur suit son contenu.
const NOTICE_WIDTH: i32 = 410;

/// Modules accessibles depuis la procédure de fenêtre de l'hôte, pour
/// l'enregistrement d'urgence à la fermeture de session.
static MODULES: OnceLock<Vec<Arc<dyn Module>>> = OnceLock::new();

thread_local! {
    /// Messages de l'application reçus par la procédure de fenêtre. Ils y
    /// arrivent même pendant une boucle modale (menu, déplacement d'une
    /// fenêtre), où la boucle principale ne tourne pas.
    static PENDING: RefCell<VecDeque<(u32, isize)>> = const { RefCell::new(VecDeque::new()) };
}

struct Tray {
    icon: TrayIcon,
    open: MenuItem,
    desktop: CheckMenuItem,
    autostart: CheckMenuItem,
    quit: MenuItem,
}

pub struct App {
    modules: Vec<Arc<dyn Module>>,
    host: HostSender,
    events: Receiver<HostEvent>,
    messages_sender: Sender<(WindowId, String)>,
    messages: Receiver<(WindowId, String)>,
    context: WebContext,
    layout: Layout,
    main: Option<WebWindow>,
    widgets: Vec<(String, WebWindow)>,
    notice: Option<WebWindow>,
    notices: Vec<Notice>,
    next_window: WindowId,
    dark: bool,
    tray: Option<Tray>,
    ticks: u32,
}

impl App {
    pub fn new() -> Result<Self> {
        let modules = crate::modules::registry();
        let _ = MODULES.set(modules.clone());

        let (event_sender, events) = channel();
        let (messages_sender, messages) = channel();

        let webview_dir = paths::webview_dir()?;
        std::fs::create_dir_all(&webview_dir).ok();

        Ok(Self {
            modules,
            host: HostSender::new(event_sender),
            events,
            messages_sender,
            messages,
            context: WebContext::new(Some(webview_dir)),
            layout: Layout::load(),
            main: None,
            widgets: Vec::new(),
            notice: None,
            notices: Vec::new(),
            next_window: 1,
            dark: webview::prefers_dark(),
            tray: None,
            ticks: 0,
        })
    }

    pub fn run(mut self) -> Result<()> {
        let host = create_host_window()?;
        ipc::set_host(host);

        for module in &self.modules {
            module.start(ModuleContext {
                host: self.host.clone(),
            });
        }

        self.tray = Some(self.build_tray()?);
        self.refresh_tray_tooltip();

        let first_run = !self.layout.initialized;
        if first_run {
            for module in &self.modules {
                for (widget, size) in module.default_widgets() {
                    self.layout.board.push(Instance {
                        id: layout::new_instance_id(),
                        widget: widget.to_string(),
                        size: size.to_string(),
                        settings: Value::Null,
                    });
                }
            }
            self.layout.initialized = true;
            self.save_layout();
        }

        let pinned = self.layout.desktop.clone();
        for widget in &pinned {
            if let Err(error) = self.open_widget_window(widget) {
                log_line!("widget {} non affiché : {error:#}", widget.instance.widget);
            }
        }

        // Premier lancement : la fenêtre se montre d'elle-même, sinon rien
        // n'indique que le programme tourne.
        let args: Vec<String> = std::env::args().collect();
        let requested_page = args
            .iter()
            .position(|arg| arg == "--open")
            .and_then(|index| args.get(index + 1).cloned());
        if first_run || requested_page.is_some() || args.iter().any(|arg| arg == "--show") {
            self.open_main(requested_page);
        }

        log_line!("Atelier démarré ({} module(s))", self.modules.len());

        let menu_events = MenuEvent::receiver();
        let tray_events = TrayIconEvent::receiver();
        let mut message = MSG::default();

        'outer: loop {
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
            if result.0 <= 0 {
                break;
            }

            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            let pending: Vec<(u32, isize)> =
                PENDING.with(|queue| queue.borrow_mut().drain(..).collect());
            for (kind, value) in pending {
                self.on_app_message(kind, value);
            }

            while let Ok(event) = tray_events.try_recv() {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    self.open_main(None);
                }
            }

            while let Ok(event) = menu_events.try_recv() {
                if self.on_menu(&event.id) {
                    unsafe { PostQuitMessage(0) };
                    continue 'outer;
                }
            }

            while let Ok((window, raw)) = self.messages.try_recv() {
                self.on_page_message(window, &raw);
            }

            while let Ok(event) = self.events.try_recv() {
                self.on_host_event(event);
            }
        }

        log_line!("arrêt demandé");
        self.save_layout();
        self.notice = None;
        self.widgets.clear();
        self.main = None;
        for module in &self.modules {
            module.stop();
        }

        Ok(())
    }

    fn allocate_id(&mut self) -> WindowId {
        let id = self.next_window;
        self.next_window += 1;
        id
    }

    fn save_layout(&self) {
        if let Err(error) = self.layout.save() {
            log_line!("disposition non enregistrée : {error:#}");
        }
    }

    fn window(&self, id: WindowId) -> Option<&WebWindow> {
        self.main
            .iter()
            .chain(self.notice.iter())
            .chain(self.widgets.iter().map(|(_, window)| window))
            .find(|window| window.id == id)
    }

    fn all_windows(&self) -> impl Iterator<Item = &WebWindow> {
        self.main
            .iter()
            .chain(self.notice.iter())
            .chain(self.widgets.iter().map(|(_, window)| window))
    }

    // --- Fenêtres ----------------------------------------------------------

    fn boot(&self, extra: Value) -> Value {
        let mut boot = json!({
            "version": env!("CARGO_PKG_VERSION"),
            "modules": self.modules.iter().map(|module| module.id()).collect::<Vec<_>>(),
            "cell": layout::CELL,
            "gap": layout::GAP,
            "sizes": layout::SIZES.iter().map(|(name, columns, rows)| json!({ "id": name, "columns": columns, "rows": rows })).collect::<Vec<_>>(),
        });
        if let (Some(target), Some(source)) = (boot.as_object_mut(), extra.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
        boot
    }

    fn open_main(&mut self, page: Option<String>) {
        if let Some(window) = &self.main {
            window.show(true);
            if let Some(page) = page {
                window.eval(&format!("Atelier.navigate({})", ipc::json(&page)));
            }
            return;
        }

        let id = self.allocate_id();
        let boot = self.boot(json!({ "kind": "shell", "page": page }));
        let spec = Spec {
            kind: Kind::Main,
            title: "Atelier".to_string(),
            width: 1180,
            height: 780,
            min_width: 860,
            min_height: 560,
            placement: Placement::Center,
            topmost: false,
            html: ui::render(Page::Shell, &self.modules, self.dark, &boot),
            dark: self.dark,
        };

        match WebWindow::create(id, spec, self.messages_sender.clone(), &mut self.context) {
            Ok(window) => {
                window.show(true);
                self.main = Some(window);
            }
            Err(error) => {
                log_line!("fenêtre principale indisponible : {error:#}");
                show_error(&format!("{error:#}"));
            }
        }
    }

    fn open_widget_window(&mut self, pinned: &Pinned) -> Result<()> {
        let (width, height) = layout::pixel_size(&pinned.instance.size);

        let on_screen = unsafe {
            !MonitorFromPoint(
                POINT {
                    x: pinned.x + 20,
                    y: pinned.y + 20,
                },
                MONITOR_DEFAULTTONULL,
            )
            .is_invalid()
        };
        let placement = if on_screen {
            Placement::At(pinned.x, pinned.y)
        } else {
            Placement::BottomRight
        };

        let id = self.allocate_id();
        let boot = self.boot(json!({
            "kind": "widget",
            "instance": pinned.instance,
            "topmost": pinned.topmost,
        }));
        let spec = Spec {
            kind: Kind::Widget,
            title: "Widget Atelier".to_string(),
            width,
            height,
            min_width: 0,
            min_height: 0,
            placement,
            topmost: pinned.topmost,
            html: ui::render(Page::Widget, &self.modules, self.dark, &boot),
            dark: self.dark,
        };

        let window = WebWindow::create(id, spec, self.messages_sender.clone(), &mut self.context)?;
        if !self.layout.desktop_hidden {
            window.show(false);
        }
        self.widgets.push((pinned.instance.id.clone(), window));
        Ok(())
    }

    fn close_widget_window(&mut self, instance: &str) {
        self.widgets.retain(|(id, _)| id != instance);
    }

    fn show_notice(&mut self) {
        let Some(current) = self.notices.first().cloned() else {
            self.notice = None;
            return;
        };

        if let Some(window) = &self.notice {
            window.eval(&format!("Atelier.emit('notice', {})", ipc::json(&current)));
            window.set_visible(true);
        } else {
            let id = self.allocate_id();
            let boot = self.boot(json!({ "kind": "notice", "notice": current }));
            let spec = Spec {
                kind: Kind::Notice,
                title: "Atelier".to_string(),
                width: NOTICE_WIDTH,
                height: 148,
                min_width: 0,
                min_height: 0,
                placement: Placement::BottomRight,
                topmost: true,
                html: ui::render(Page::Notice, &self.modules, self.dark, &boot),
                dark: self.dark,
            };

            match WebWindow::create(id, spec, self.messages_sender.clone(), &mut self.context) {
                Ok(window) => {
                    window.show(false);
                    self.notice = Some(window);
                }
                Err(error) => log_line!("notification non affichée : {error:#}"),
            }
        }

        let sound = match current.tone {
            "critical" => MB_ICONHAND,
            "warning" => MB_ICONEXCLAMATION,
            _ => MB_ICONASTERISK,
        };
        unsafe {
            let _ = MessageBeep(sound);
        }
    }

    fn remove_notice(&mut self, id: &str) {
        let was_current = self.notices.first().is_some_and(|notice| notice.id == id);
        self.notices.retain(|notice| notice.id != id);
        if was_current {
            if self.notices.is_empty() {
                self.notice = None;
            } else {
                self.show_notice();
            }
        }
    }

    // --- Messages ----------------------------------------------------------

    fn on_app_message(&mut self, kind: u32, value: isize) {
        match kind {
            ipc::WM_APP_SHOW => self.open_main(None),

            ipc::WM_APP_WINDOW_CLOSED => {
                let hwnd = HWND(value as *mut _);
                if self.main.as_ref().is_some_and(|window| window.hwnd == hwnd) {
                    self.main = None;
                } else if self
                    .notice
                    .as_ref()
                    .is_some_and(|window| window.hwnd == hwnd)
                {
                    if let Some(id) = self.notices.first().map(|notice| notice.id.clone()) {
                        self.remove_notice(&id);
                    }
                    self.notice = None;
                } else if let Some(instance) = self
                    .widgets
                    .iter()
                    .find(|(_, window)| window.hwnd == hwnd)
                    .map(|(id, _)| id.clone())
                {
                    // Alt+F4 sur un widget du bureau : on le retire.
                    self.close_widget_window(&instance);
                    self.layout
                        .desktop
                        .retain(|pinned| pinned.instance.id != instance);
                    self.save_layout();
                }
            }

            ipc::WM_APP_WINDOW_MOVED => {
                let hwnd = HWND(value as *mut _);
                if let Some((instance, rect)) = self
                    .widgets
                    .iter()
                    .find(|(_, window)| window.hwnd == hwnd)
                    .map(|(id, window)| (id.clone(), window.rect()))
                    && let Some(pinned) = self.layout.pinned_mut(&instance)
                {
                    pinned.x = rect.left;
                    pinned.y = rect.top;
                    self.save_layout();
                }
            }

            ipc::WM_APP_THEME => {
                let dark = webview::prefers_dark();
                if dark != self.dark {
                    self.dark = dark;
                    for window in self.all_windows() {
                        window.set_dark(dark);
                        window.eval(&format!("Atelier.setTheme({dark})"));
                    }
                }
            }

            ipc::WM_APP_TIMER => {
                self.ticks += 1;
                if self.ticks.is_multiple_of(30) {
                    self.refresh_tray_tooltip();
                }
            }

            _ => {}
        }
    }

    fn on_host_event(&mut self, event: HostEvent) {
        match event {
            HostEvent::Reply {
                window,
                request,
                result,
            } => {
                if let Some(target) = self.window(window) {
                    let script = match result {
                        Ok(value) => {
                            format!("Atelier.resolve({request}, true, {})", ipc::json(&value))
                        }
                        Err(error) => {
                            format!("Atelier.resolve({request}, false, {})", ipc::json(&error))
                        }
                    };
                    target.eval(&script);
                }
            }
            HostEvent::Broadcast { event, payload } => {
                let script = format!(
                    "Atelier.emit({}, {})",
                    ipc::json(&event),
                    ipc::json(&payload)
                );
                for window in self.all_windows() {
                    window.eval(&script);
                }
            }
            HostEvent::Notice(notice) => {
                match self
                    .notices
                    .iter_mut()
                    .find(|existing| existing.id == notice.id)
                {
                    Some(existing) => *existing = notice.clone(),
                    None => self.notices.push(notice.clone()),
                }
                // La notification neuve passe devant : c'est la plus urgente.
                if let Some(position) = self
                    .notices
                    .iter()
                    .position(|existing| existing.id == notice.id)
                {
                    let item = self.notices.remove(position);
                    self.notices.insert(0, item);
                }
                self.show_notice();
            }
            HostEvent::DismissNotice(id) => self.remove_notice(&id),
        }
    }

    fn on_page_message(&mut self, window: WindowId, raw: &str) {
        let Some(request) = ipc::parse_request(raw) else {
            log_line!("message illisible depuis une page : {raw}");
            return;
        };

        let reply = Responder::new(window, request.id, self.host.clone());

        if request.target == "core" {
            let result = self.handle_core(window, &request.method, request.params);
            reply.result(result);
        } else if let Some(module) = self
            .modules
            .iter()
            .find(|module| module.id() == request.target)
        {
            module.handle(&request.method, request.params, reply);
        } else {
            reply.err(format!("module inconnu : {}", request.target));
        }
    }

    fn widget_instance_of(&self, window: WindowId) -> Option<String> {
        self.widgets
            .iter()
            .find(|(_, candidate)| candidate.id == window)
            .map(|(id, _)| id.clone())
    }

    /// Requêtes adressées à l'hôte lui-même.
    fn handle_core(&mut self, window: WindowId, method: &str, params: Value) -> Result<Value> {
        let string = |name: &str| -> Result<String> {
            params
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| anyhow!("paramètre manquant : {name}"))
        };

        match method {
            "layout" => Ok(serde_json::to_value(&self.layout)?),

            "layout.board" => {
                let board: Vec<Instance> =
                    serde_json::from_value(params.get("board").cloned().unwrap_or(Value::Null))
                        .context("disposition invalide")?;
                self.layout.board = board;
                self.save_layout();
                Ok(Value::Null)
            }

            "widget.pin" => {
                let mut instance: Instance =
                    serde_json::from_value(params.get("instance").cloned().unwrap_or(Value::Null))
                        .context("widget invalide")?;
                instance.id = layout::new_instance_id();

                let pinned = Pinned {
                    instance,
                    x: i32::MIN,
                    y: i32::MIN,
                    topmost: false,
                };
                self.open_widget_window(&pinned)?;

                // En cascade depuis le coin, pour ne pas empiler les widgets.
                let mut pinned = pinned;
                if let Some((_, window)) = self.widgets.last() {
                    let offset = (self.layout.desktop.len() as i32 % 6) * 28;
                    let rect = window.rect();
                    unsafe {
                        let _ = SetWindowPos(
                            window.hwnd,
                            None,
                            rect.left - offset,
                            rect.top - offset,
                            0,
                            0,
                            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                    pinned.x = rect.left - offset;
                    pinned.y = rect.top - offset;
                    if self.layout.desktop_hidden {
                        self.layout.desktop_hidden = false;
                        self.set_desktop_visible(true);
                    }
                }

                self.layout.desktop.push(pinned.clone());
                self.save_layout();
                Ok(serde_json::to_value(&pinned)?)
            }

            "widget.unpin" => {
                let id = match params.get("id").and_then(Value::as_str) {
                    Some(id) => id.to_string(),
                    None => self
                        .widget_instance_of(window)
                        .ok_or_else(|| anyhow!("widget introuvable"))?,
                };
                self.layout
                    .desktop
                    .retain(|pinned| pinned.instance.id != id);
                self.save_layout();
                self.close_widget_window(&id);
                Ok(Value::Null)
            }

            "widget.update" => {
                let id = self
                    .widget_instance_of(window)
                    .ok_or_else(|| anyhow!("ce n'est pas un widget du bureau"))?;
                let pinned = self
                    .layout
                    .pinned_mut(&id)
                    .ok_or_else(|| anyhow!("widget introuvable"))?;

                if let Some(size) = params.get("size").and_then(Value::as_str) {
                    pinned.instance.size = size.to_string();
                }
                if let Some(topmost) = params.get("topmost").and_then(Value::as_bool) {
                    pinned.topmost = topmost;
                }
                if let Some(settings) = params.get("settings") {
                    pinned.instance.settings = settings.clone();
                }

                let snapshot = pinned.clone();
                if let Some((_, window)) = self
                    .widgets
                    .iter_mut()
                    .find(|(candidate, _)| *candidate == id)
                {
                    let (width, height) = layout::pixel_size(&snapshot.instance.size);
                    window.resize(width, height);
                    window.set_topmost(snapshot.topmost);
                }
                self.save_layout();
                Ok(serde_json::to_value(&snapshot)?)
            }

            "widget.drag" => {
                if let Some((_, window)) = self
                    .widgets
                    .iter()
                    .find(|(_, candidate)| candidate.id == window)
                {
                    window.begin_drag();
                }
                Ok(Value::Null)
            }

            // Taille logique de la fenêtre principale : sert à l'audit de mise
            // en page, qui passe chaque écran à plusieurs largeurs.
            "window.resize" => {
                let dimension = |name: &str, min: i64| -> Result<i32> {
                    let value = params
                        .get(name)
                        .and_then(Value::as_i64)
                        .ok_or_else(|| anyhow!("paramètre manquant : {name}"))?;
                    Ok(value.clamp(min, 4000) as i32)
                };
                let (width, height) = (dimension("width", 860)?, dimension("height", 560)?);
                let window = self
                    .main
                    .as_mut()
                    .ok_or_else(|| anyhow!("fenêtre principale fermée"))?;
                window.resize(width, height);
                Ok(json!({ "width": width, "height": height }))
            }

            "desktop.visible" => {
                let visible = params
                    .get("visible")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                self.set_desktop_visible(visible);
                Ok(json!({ "visible": visible }))
            }

            "open" => {
                let page = params
                    .get("page")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.open_main(page);
                Ok(Value::Null)
            }

            "notice.action" => {
                let id = string("id")?;
                let action = string("action")?;
                if let Some(notice) = self.notices.iter().find(|notice| notice.id == id).cloned()
                    && let Some(module) = self
                        .modules
                        .iter()
                        .find(|module| module.id() == notice.module)
                {
                    module.notice_action(&id, &action);
                }
                self.remove_notice(&id);
                Ok(Value::Null)
            }

            // Notification d'essai, pour l'audit de mise en page : textes et
            // compte à rebours fournis par l'appelant.
            "notice.preview" => {
                let text = |name: &str, fallback: &str| {
                    params
                        .get(name)
                        .and_then(Value::as_str)
                        .unwrap_or(fallback)
                        .to_string()
                };
                let notice = Notice {
                    id: "preview".to_string(),
                    module: "core".to_string(),
                    title: text("title", "Notification d'essai"),
                    body: text("body", "Aperçu de la mise en page des notifications."),
                    icon: None,
                    glyph: None,
                    tone: "critical",
                    countdown: params
                        .get("countdown")
                        .and_then(Value::as_u64)
                        .map(|seconds| seconds as u32),
                    timeout: None,
                    actions: vec![
                        ipc::NoticeAction::new("extend", "+\u{a0}15\u{a0}min", false),
                        ipc::NoticeAction::new("dismiss", "Fermer maintenant", true),
                    ],
                };
                self.on_host_event(HostEvent::Notice(notice));
                Ok(Value::Null)
            }

            // La notification annonce la hauteur de son contenu : la fenêtre
            // s'y ajuste, toujours ancrée au coin de l'écran.
            "notice.fit" => {
                let height = params
                    .get("height")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| anyhow!("paramètre manquant : height"))?
                    .clamp(80, 480) as i32;
                if let Some(window) = self.notice.as_mut() {
                    window.reposition(NOTICE_WIDTH, height, &Placement::BottomRight);
                }
                Ok(Value::Null)
            }

            "notice.dismiss" => {
                let id = string("id")?;
                self.remove_notice(&id);
                Ok(Value::Null)
            }

            "status" => {
                let status = install::status().ok();
                Ok(json!({
                    "autostart": autostart::is_enabled(),
                    "autostartTarget": autostart::registered_target(),
                    "install": status,
                    "portable": paths::is_portable(),
                    "dataDir": paths::data_dir().ok(),
                    "desktopHidden": self.layout.desktop_hidden,
                }))
            }

            "autostart.set" => {
                let wanted = params
                    .get("value")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                autostart::set(wanted)?;
                if let Some(tray) = &self.tray {
                    tray.autostart.set_checked(autostart::is_enabled());
                }
                Ok(json!({ "autostart": autostart::is_enabled() }))
            }

            "install" => {
                let path = install::install()?;
                Ok(json!({ "path": path }))
            }

            "uninstall" => match install::uninstall()? {
                install::Removal::Deferred => {
                    unsafe { PostQuitMessage(0) };
                    Ok(json!({ "deferred": true }))
                }
                _ => Ok(json!({ "deferred": false })),
            },

            "open.data" => {
                let directory = paths::data_dir()?;
                std::fs::create_dir_all(&directory)?;
                crate::shell::open(&directory)?;
                Ok(Value::Null)
            }

            "open.log" => {
                let path = paths::log_path()?;
                if !path.exists() {
                    crate::logging::log("journal créé");
                }
                crate::shell::open(&path)?;
                Ok(Value::Null)
            }

            "quit" => {
                unsafe { PostQuitMessage(0) };
                Ok(Value::Null)
            }

            other => Err(anyhow!("méthode inconnue : {other}")),
        }
    }

    fn set_desktop_visible(&mut self, visible: bool) {
        self.layout.desktop_hidden = !visible;
        for (_, window) in &self.widgets {
            window.set_visible(visible);
        }
        if let Some(tray) = &self.tray {
            tray.desktop.set_checked(visible);
        }
        self.save_layout();
    }

    // --- Zone de notification ------------------------------------------------

    fn build_tray(&self) -> Result<Tray> {
        let open = MenuItem::new("Ouvrir Atelier", true, None);
        let desktop = CheckMenuItem::new(
            "Afficher les widgets du bureau",
            true,
            !self.layout.desktop_hidden,
            None,
        );
        let autostart_item =
            CheckMenuItem::new("Démarrer avec Windows", true, autostart::is_enabled(), None);
        let quit = MenuItem::new("Quitter", true, None);

        let menu = Menu::new();
        menu.append_items(&[
            &open,
            &desktop,
            &PredefinedMenuItem::separator(),
            &autostart_item,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .context("construction du menu")?;

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Atelier")
            .with_icon(load_tray_icon()?)
            // Convention Windows : clic gauche pour l'action principale, clic
            // droit pour le menu.
            .with_menu_on_left_click(false)
            .build()
            .context("création de l'icône de la zone de notification")?;

        Ok(Tray {
            icon,
            open,
            desktop,
            autostart: autostart_item,
            quit,
        })
    }

    fn refresh_tray_tooltip(&self) {
        let Some(tray) = &self.tray else { return };
        let mut lines = vec!["Atelier".to_string()];
        lines.extend(
            self.modules
                .iter()
                .filter_map(|module| module.tray_status()),
        );
        let _ = tray.icon.set_tooltip(Some(lines.join("\n")));
    }

    /// Retourne `true` s'il faut quitter.
    fn on_menu(&mut self, id: &tray_icon::menu::MenuId) -> bool {
        let Some(tray) = &self.tray else { return false };

        if id == tray.open.id() {
            self.open_main(None);
        } else if id == tray.desktop.id() {
            let visible = tray.desktop.is_checked();
            self.set_desktop_visible(visible);
        } else if id == tray.autostart.id() {
            let wanted = tray.autostart.is_checked();
            if let Err(error) = autostart::set(wanted) {
                log_line!("démarrage automatique non modifié : {error:#}");
            }
            tray.autostart.set_checked(autostart::is_enabled());
        } else if id == tray.quit.id() {
            return true;
        }

        false
    }
}

/// Icône de la zone de notification, dessinée au pixel près pour chaque
/// échelle d'affichage (`scripts/logo/build_logo.py`) : Windows reçoit la
/// taille exacte qu'il affiche, au lieu de réduire une grande image.
fn load_tray_icon() -> Result<tray_icon::Icon> {
    const SIZES: [(i32, &[u8]); 4] = [
        (16, include_bytes!("../assets/tray-16.png")),
        (20, include_bytes!("../assets/tray-20.png")),
        (24, include_bytes!("../assets/tray-24.png")),
        (32, include_bytes!("../assets/tray-32.png")),
    ];

    // Taille des petites icônes à l'échelle du système : 16 px à 100 %,
    // 20 px à 125 %, 24 px à 150 %, 32 px à 200 %.
    let wanted = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let (size, png) = SIZES
        .iter()
        .copied()
        .find(|(size, _)| *size >= wanted)
        .unwrap_or(SIZES[SIZES.len() - 1]);

    let mut image = image::load_from_memory(png)
        .context("décodage de l'icône embarquée")?
        .into_rgba8();
    // Échelle intermédiaire (175 %…) : la taille dessinée la plus proche,
    // ajustée.
    if size != wanted {
        let wanted = wanted as u32;
        image = image::imageops::resize(
            &image,
            wanted,
            wanted,
            image::imageops::FilterType::Lanczos3,
        );
    }

    let (width, height) = image.dimensions();
    tray_icon::Icon::from_rgba(image.into_raw(), width, height)
        .context("construction de l'icône de la zone de notification")
}

/// Empreinte du dossier de données : une instance par dossier.
pub fn instance_identity() -> Result<String> {
    let directory = paths::data_dir()?.to_string_lossy().to_lowercase();
    // FNV-1a : stable d'une version de Rust à l'autre, contrairement au
    // hachage de la bibliothèque standard.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in directory.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    Ok(format!("{hash:016x}"))
}

/// Titre de la fenêtre hôte, par lequel une seconde instance la retrouve.
pub fn host_title(identity: &str) -> String {
    format!("Atelier {identity}")
}

fn create_host_window() -> Result<HWND> {
    webview::register_class(HOST_CLASS, Some(host_proc))?;
    let instance = unsafe { GetModuleHandleW(None) }.context("module courant introuvable")?;
    let title = crate::to_wide(&host_title(&instance_identity()?));

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            HOST_CLASS,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .context("création de la fenêtre hôte")?;

    unsafe {
        SetTimer(Some(hwnd), TIMER_ID, 2000, None);
    }

    Ok(hwnd)
}

fn flush_modules() {
    if let Some(modules) = MODULES.get() {
        for module in modules {
            module.flush();
        }
    }
}

unsafe extern "system" fn host_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        ipc::WM_APP_WAKE
        | ipc::WM_APP_SHOW
        | ipc::WM_APP_WINDOW_CLOSED
        | ipc::WM_APP_WINDOW_MOVED
        | ipc::WM_APP_THEME => {
            PENDING.with(|queue| queue.borrow_mut().push_back((message, lparam.0)));
            LRESULT(0)
        }
        // La minuterie garde la boucle vivante et rythme les rafraîchissements.
        WM_TIMER => {
            PENDING.with(|queue| queue.borrow_mut().push_back((ipc::WM_APP_TIMER, 0)));
            LRESULT(0)
        }
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                log_line!("fermeture de session : enregistrement");
                flush_modules();
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            // PBT_APMSUSPEND : la machine s'endort, peut-être pour longtemps.
            if wparam.0 == 0x4 {
                flush_modules();
            }
            LRESULT(1)
        }
        WM_SETTINGCHANGE => {
            let name = lparam.0 as *const u16;
            if !name.is_null() {
                let text = unsafe { PCWSTR(name).to_string() }.unwrap_or_default();
                if text == "ImmersiveColorSet" {
                    PENDING.with(|queue| queue.borrow_mut().push_back((ipc::WM_APP_THEME, 0)));
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

pub fn show_error(message: &str) {
    let text = crate::to_wide(message);
    let title = crate::to_wide("Atelier");
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_ICONERROR | MB_OK,
        );
    }
}
