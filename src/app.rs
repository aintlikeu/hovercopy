//! Tray app: global hotkey → capture around the cursor → OCR on a worker thread →
//! clipboard + a toast near the cursor that never takes focus from the window under it.

use std::cell::RefCell;
use std::mem::{size_of, zeroed};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io, thread};

use hover_copy::bgr::BgrImage;
use hover_copy::finder::{capture_region, CAPTURE_H, CAPTURE_W, DET_SCALE_96};
use hover_copy::ocr::Ocr;
use hover_copy::settings::{Hotkey, Settings};
use hover_copy::Outcome;
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmFlush;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Controls::{InitCommonControlsEx, ICC_HOTKEY_CLASS, INITCOMMONCONTROLSEX};
use windows_sys::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT};
use windows_sys::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::settings_window;
use crate::win::{self, wide};

const WM_APP_TRAY: u32 = WM_APP + 1;
const WM_APP_RESULT: u32 = WM_APP + 2;
const HOTKEY_ID: i32 = 1;
const TRAY_ID: u32 = 1;
const ICON_RESOURCE: usize = 1;
const TOAST_TIMER: usize = 1;
const TOAST_MS: u32 = 1500;
const CMD_SETTINGS: usize = 1;
const CMD_EXIT: usize = 2;

/// "TaskbarCreated" message id: Explorer restarted and tray icons must be added again.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

struct Job {
    capture: BgrImage,
    cursor: (i32, i32),
    scale: f32,
}

type JobResult = Result<Option<String>, String>;

struct Ui {
    main: HWND,
    toast: HWND,
    icon: HICON,
    settings: Settings,
    /// The hotkey is registered; another program may hold it.
    hotkey_active: bool,
    settings_path: PathBuf,
    debug_dir: PathBuf,
    /// `settings.save_debug`, shared with the worker.
    save_debug: Arc<AtomicBool>,
    jobs: Sender<Job>,
    results: Receiver<JobResult>,
    /// A capture is being recognized; further hotkey presses are ignored.
    busy: bool,
    toast_text: Vec<u16>,
    toast_ok: bool,
    font: HFONT,
    font_dpi: u32,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

/// Short access to the UI state. `None` while it is borrowed further up the stack: window
/// procedures re-enter during modal loops (menus, dialogs, message boxes), so never call such
/// APIs inside `f`.
fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(f))
}

pub fn run() {
    unsafe {
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) == 0 {
            SetProcessDPIAware();
        }
        // Stays open until the process exits.
        CreateMutexW(null(), 0, wide("Local\\HoverCopy.SingleInstance").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            win::message_box(null_mut(), "HoverCopy уже запущен — его значок в трее, рядом с часами.", false);
            return;
        }
    }

    let (settings_path, debug_dir) = data_paths();
    let (settings, settings_error) = match load_settings(&settings_path) {
        Ok(s) => (s, None),
        Err(e) => (Settings::default(), Some(e)),
    };

    unsafe {
        let controls = INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_HOTKEY_CLASS };
        InitCommonControlsEx(&controls);
        let instance = GetModuleHandleW(null());
        TASKBAR_CREATED.store(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()), Ordering::Relaxed);
        let main_class = wide("HoverCopyMain");
        let toast_class = wide("HoverCopyToast");
        for (class, proc_) in [(&main_class, main_proc as WndProc), (&toast_class, toast_proc as WndProc)] {
            let mut wc: WNDCLASSW = zeroed();
            wc.lpfnWndProc = Some(proc_);
            wc.hInstance = instance;
            wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
            wc.lpszClassName = class.as_ptr();
            RegisterClassW(&wc);
        }
        // Never shown: receives the hotkey, tray and worker messages, owns the settings dialog.
        let main = CreateWindowExW(0, main_class.as_ptr(), wide("HoverCopy").as_ptr(), WS_OVERLAPPED, 0, 0, 0, 0, null_mut(), null_mut(), instance, null());
        // Click-through, never activated, so the window under the cursor keeps keyboard focus.
        let toast = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            toast_class.as_ptr(),
            null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if main.is_null() || toast.is_null() {
            win::message_box(null_mut(), &format!("Не удалось создать окно (ошибка {}).", GetLastError()), true);
            return;
        }
        SetLayeredWindowAttributes(toast, 0, 235, LWA_ALPHA);

        let hotkey_active = register_hotkey(main, &settings.hotkey);
        let save_debug = Arc::new(AtomicBool::new(settings.save_debug));
        let (jobs, job_rx) = channel();
        let (result_tx, results) = channel();
        let (main_addr, worker_debug, worker_dir) = (main as isize, save_debug.clone(), debug_dir.clone());
        thread::spawn(move || worker(job_rx, result_tx, main_addr, worker_debug, worker_dir));

        let mut icon = LoadIconW(instance, ICON_RESOURCE as *const u16);
        if icon.is_null() {
            icon = LoadIconW(null_mut(), IDI_APPLICATION);
        }
        let hotkey_label = settings.hotkey.label.clone();
        let ui = Ui {
            main,
            toast,
            icon,
            settings,
            hotkey_active,
            settings_path: settings_path.clone(),
            debug_dir,
            save_debug,
            jobs,
            results,
            busy: false,
            toast_text: Vec::new(),
            toast_ok: true,
            font: null_mut(),
            font_dpi: 0,
        };
        UI.with(|cell| *cell.borrow_mut() = Some(ui));
        tray(NIM_ADD);

        if let Some(e) = settings_error {
            let text = format!("Ошибка в {}:\n{e}\n\nПока действуют настройки по умолчанию.", settings_path.display());
            win::message_box(null_mut(), &text, true);
        }
        if !hotkey_active {
            win::message_box(null_mut(), &format!("Сочетание {hotkey_label} уже занято другой программой. Выберите другое."), true);
            open_settings();
        }

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => on_hotkey(),
        WM_APP_RESULT => on_result(),
        WM_APP_TRAY => {
            if matches!((lparam & 0xFFFF) as u32, WM_LBUTTONUP | WM_RBUTTONUP | WM_CONTEXTMENU) {
                show_menu(hwnd);
            }
        }
        WM_DESTROY => {
            UnregisterHotKey(hwnd, HOTKEY_ID);
            tray(NIM_DELETE);
            PostQuitMessage(0);
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => tray(NIM_ADD),
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    0
}

unsafe extern "system" fn toast_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if let Some((text, ok, font)) = with_ui(|u| (u.toast_text.clone(), u.toast_ok, u.font)) {
                let mut rc: RECT = zeroed();
                GetClientRect(hwnd, &mut rc);
                let background = CreateSolidBrush(rgb(32, 32, 32));
                FillRect(hdc, &rc, background);
                DeleteObject(background);
                SetBkMode(hdc, TRANSPARENT as _);
                SetTextColor(hdc, if ok { rgb(120, 230, 140) } else { rgb(255, 140, 140) });
                let old = SelectObject(hdc, font);
                DrawTextW(hdc, text.as_ptr(), text.len() as i32, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
                SelectObject(hdc, old);
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_TIMER => {
            KillTimer(hwnd, TOAST_TIMER);
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn on_hotkey() {
    let Some((false, toast)) = with_ui(|u| (u.busy, u.toast)) else {
        return;
    };
    // The previous toast may still cover nearby text: take it off the screen and wait for the
    // next DWM frame, otherwise the capture still shows it.
    unsafe {
        if IsWindowVisible(toast) != 0 {
            KillTimer(toast, TOAST_TIMER);
            ShowWindow(toast, SW_HIDE);
            DwmFlush();
        }
    }
    let (cx, cy) = win::cursor_pos();
    let (region, scale) = capture_region(win::virtual_screen(), cx, cy, win::monitor_at(cx, cy).dpi);
    let capture = match win::capture(region) {
        Ok(c) => c,
        Err(e) => return show_toast(&format!("Не удалось снять экран: {e}"), false),
    };
    let job = Job { capture, cursor: (cx - region.x, cy - region.y), scale };
    if with_ui(|u| {
        u.busy = u.jobs.send(job).is_ok();
        u.busy
    }) == Some(false)
    {
        show_toast("Распознавание остановилось — перезапустите HoverCopy", false);
    }
}

fn on_result() {
    let Some(Ok(result)) = with_ui(|u| {
        let r = u.results.try_recv();
        if r.is_ok() {
            u.busy = false;
        }
        r
    }) else {
        return;
    };
    let Some((main, show_copied)) = with_ui(|u| (u.main, u.settings.show_toast)) else {
        return;
    };
    match result {
        Ok(Some(word)) => match win::set_clipboard(main, &word) {
            Ok(()) if show_copied => show_toast(&format!("Скопировано: {word}"), true),
            Ok(()) => {}
            Err(e) => show_toast(&format!("{word} — не скопировано: {e}"), false),
        },
        Ok(None) => show_toast("Текст не найден", false),
        Err(e) => show_toast(&format!("Ошибка: {e}"), false),
    }
}

/// Shows `text` below-right of the cursor for TOAST_MS; a new toast replaces the current one.
fn show_toast(text: &str, ok: bool) {
    let (cx, cy) = win::cursor_pos();
    let monitor = win::monitor_at(cx, cy);
    let Some((toast, font, text)) = with_ui(|u| {
        if u.font.is_null() || u.font_dpi != monitor.dpi {
            if !u.font.is_null() {
                unsafe { DeleteObject(u.font) };
            }
            u.font = win::create_font(monitor.dpi);
            u.font_dpi = monitor.dpi;
        }
        u.toast_text = text.encode_utf16().collect();
        u.toast_ok = ok;
        (u.toast, u.font, u.toast_text.clone())
    }) else {
        return;
    };
    let px = |v: i32| v * monitor.dpi as i32 / 96;
    unsafe {
        let mut rc: RECT = zeroed();
        let hdc = GetDC(toast);
        let old = SelectObject(hdc, font);
        DrawTextW(hdc, text.as_ptr(), text.len() as i32, &mut rc, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
        SelectObject(hdc, old);
        ReleaseDC(toast, hdc);

        let (w, h) = (rc.right - rc.left + px(20), rc.bottom - rc.top + px(12));
        let work = monitor.work;
        let x = (cx + px(16)).min(work.right() - w).max(work.x);
        let y = (cy + px(20)).min(work.bottom() - h).max(work.y);
        SetWindowPos(toast, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        InvalidateRect(toast, null(), 1);
        SetTimer(toast, TOAST_TIMER, TOAST_MS, None); // re-arming restarts the countdown
    }
}

fn show_menu(hwnd: HWND) {
    let Some(tip) = with_ui(|u| tip(u)) else {
        return;
    };
    unsafe {
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide(&tip).as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, CMD_SETTINGS, wide("Настройки…").as_ptr());
        AppendMenuW(menu, MF_STRING, CMD_EXIT, wide("Выход").as_ptr());
        let (x, y) = win::cursor_pos();
        // Documented quirk: without these two calls the menu does not close on an outside click.
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY, x, y, 0, hwnd, null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        match cmd as usize {
            CMD_SETTINGS => open_settings(),
            CMD_EXIT => {
                DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}

/// Runs the settings dialog. The hotkey is released while it is open, so that the hotkey field
/// can receive it; Save registers the new one, Cancel the old one again.
fn open_settings() {
    if settings_window::focus_if_open() {
        return;
    }
    let Some((main, current, path, debug_dir)) = with_ui(|u| (u.main, u.settings.clone(), u.settings_path.clone(), u.debug_dir.clone())) else {
        return;
    };
    unsafe { UnregisterHotKey(main, HOTKEY_ID) };
    let mut apply = |new: &Settings| -> Result<(), String> {
        if !register_hotkey(main, &new.hotkey) {
            return Err(format!("Сочетание {} уже занято другой программой. Выберите другое.", new.hotkey.label));
        }
        save_settings(&path, new).map_err(|e| {
            unsafe { UnregisterHotKey(main, HOTKEY_ID) };
            format!("Не удалось сохранить {}:\n{e}", path.display())
        })
    };
    let saved = settings_window::show(main, &current, &path, &debug_dir, &mut apply);
    if unsafe { IsWindow(main) } == 0 {
        return; // "Выход" was chosen while the dialog was open
    }
    let active = saved.is_some() || register_hotkey(main, &current.hotkey);
    with_ui(|u| {
        if let Some(new) = saved {
            u.save_debug.store(new.save_debug, Ordering::Relaxed);
            u.settings = new;
        }
        u.hotkey_active = active;
    });
    tray(NIM_MODIFY);
    if !active {
        win::message_box(null_mut(), &format!("Сочетание {} уже занято другой программой.", current.hotkey.label), true);
    }
}

fn register_hotkey(main: HWND, hotkey: &Hotkey) -> bool {
    unsafe { RegisterHotKey(main, HOTKEY_ID, hotkey.modifiers | MOD_NOREPEAT, hotkey.vk) != 0 }
}

fn tip(u: &Ui) -> String {
    if u.hotkey_active {
        format!("HoverCopy — копировать текст под курсором: {}", u.settings.hotkey.label)
    } else {
        "HoverCopy — сочетание клавиш не работает, откройте «Настройки»".to_owned()
    }
}

fn tray(op: u32) {
    let Some((main, icon, tip)) = with_ui(|u| (u.main, u.icon, tip(u))) else {
        return;
    };
    unsafe {
        let mut nid: NOTIFYICONDATAW = zeroed();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = main;
        nid.uID = TRAY_ID;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_APP_TRAY;
        nid.hIcon = icon;
        for (dst, src) in nid.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
            *dst = src;
        }
        Shell_NotifyIconW(op, &nid);
    }
}

/// settings.ini in the roaming AppData; debug captures, which can pile up, in the local one.
fn data_paths() -> (PathBuf, PathBuf) {
    let app_dir = |var: &str| std::env::var_os(var).map(|d| PathBuf::from(d).join("HoverCopy"));
    let exe_dir = || std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."));
    let settings_dir = app_dir("APPDATA").unwrap_or_else(exe_dir);
    let debug_dir = app_dir("LOCALAPPDATA").unwrap_or_else(|| settings_dir.clone()).join("debug");
    (settings_dir.join("settings.ini"), debug_dir)
}

/// A missing file is created with defaults; a broken one is reported and left as it is.
fn load_settings(path: &Path) -> Result<Settings, String> {
    match fs::read(path) {
        // Lossy: a file saved in a legacy code page only garbles the comments, keys are ASCII.
        Ok(bytes) => Settings::parse(&String::from_utf8_lossy(&bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let _ = save_settings(path, &Settings::default());
            Ok(Settings::default())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Writes a temporary file and renames it over the old one, so a failure never leaves half a file.
fn save_settings(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("ini.tmp");
    fs::write(&temp, settings.to_ini())?;
    fs::rename(&temp, path)
}

fn worker(jobs: Receiver<Job>, results: Sender<JobResult>, main: isize, save_debug: Arc<AtomicBool>, debug_dir: PathBuf) {
    // Loading here, right at startup, means the first hotkey press does not wait for it. The
    // detection input is the same on every monitor (see capture_region), so prepare it too.
    let mut ocr = Ocr::new()
        .and_then(|mut ocr| ocr.prepare_detection(CAPTURE_W as usize, CAPTURE_H as usize, DET_SCALE_96).map(|_| ocr))
        .map_err(|e| format!("не загрузились модели OCR: {e}"));
    for job in jobs {
        let result = match ocr.as_mut() {
            Err(e) => Err(e.clone()),
            Ok(ocr) => catch_unwind(AssertUnwindSafe(|| -> JobResult {
                let out = hover_copy::find_word(ocr, &job.capture, job.cursor, job.scale).map_err(|e| e.to_string())?;
                if save_debug.load(Ordering::Relaxed) {
                    write_debug(&debug_dir, &job, &out);
                }
                Ok(out.word)
            }))
            .unwrap_or_else(|_| Err("сбой распознавания".into())),
        };
        if results.send(result).is_err() {
            break;
        }
        unsafe { PostMessageW(main as HWND, WM_APP_RESULT, 0, 0) };
    }
}

fn write_debug(dir: &Path, job: &Job, out: &Outcome) {
    let _ = fs::create_dir_all(dir);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let _ = job.capture.save_png(&dir.join(format!("{stamp}.png")));
    let text = format!(
        "cursor: {:?}\nscale: {}\nword: {:?}\nline: {:?}\nread: {:?}\nlines: {:?}\n",
        job.cursor, job.scale, out.word, out.line, out.read, out.lines
    );
    let _ = fs::write(dir.join(format!("{stamp}.txt")), text);
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
