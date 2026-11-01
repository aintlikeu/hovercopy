//! Modal settings dialog (dialog 100 in assets/app.rc): hotkey, language, toast and debug
//! switches. It is shown in the current language; a new language applies after Save.

use std::path::Path;
use std::ptr::null;
use std::sync::atomic::{AtomicIsize, Ordering};

use hover_copy::i18n::Lang;
use hover_copy::settings::{Hotkey, Settings};
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{CheckDlgButton, IsDlgButtonChecked, BST_CHECKED, BST_UNCHECKED, HKM_GETHOTKEY, HKM_SETHOTKEY};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::win::{self, wide};

// Ids from assets/app.rc.
const DIALOG: usize = 100;
const IDC_HOTKEY: i32 = 101;
const IDC_SHOW_TOAST: i32 = 102;
const IDC_SAVE_DEBUG: i32 = 103;
const IDC_OPEN_DEBUG: i32 = 104;
const IDC_LOCATION: i32 = 105;
const IDC_HOTKEY_LABEL: i32 = 106;
const IDC_HOTKEY_HINT: i32 = 107;
const IDC_LANGUAGE: i32 = 108;
const IDC_LANGUAGE_LABEL: i32 = 109;

/// The open dialog, if any.
static OPEN: AtomicIsize = AtomicIsize::new(0);

/// Called with the settings to save; an error keeps the dialog open and is shown to the user.
pub type Apply<'a> = dyn FnMut(&Settings) -> Result<(), String> + 'a;

struct State<'a> {
    current: &'a Settings,
    settings_path: &'a Path,
    debug_dir: &'a Path,
    apply: &'a mut Apply<'a>,
    /// What the hotkey field showed at start: the field cannot show Win, so an untouched field
    /// keeps the current hotkey as it is.
    initial_field: u16,
    saved: Option<Settings>,
}

/// Brings the dialog forward if it is open; true if it was.
pub fn focus_if_open() -> bool {
    let open = OPEN.load(Ordering::Relaxed);
    if open != 0 {
        unsafe { SetForegroundWindow(open as HWND) };
    }
    open != 0
}

/// Shows the dialog until Save succeeds or the user cancels; returns the saved settings.
pub fn show<'a>(owner: HWND, current: &'a Settings, settings_path: &'a Path, debug_dir: &'a Path, apply: &'a mut Apply<'a>) -> Option<Settings> {
    let mut state = State { current, settings_path, debug_dir, apply, initial_field: 0, saved: None };
    unsafe {
        DialogBoxParamW(GetModuleHandleW(null()), DIALOG as *const u16, owner, Some(dialog_proc), &mut state as *mut State as LPARAM);
    }
    state.saved
}

unsafe extern "system" fn dialog_proc(dlg: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    if msg == WM_INITDIALOG {
        SetWindowLongPtrW(dlg, GWLP_USERDATA, lparam);
        OPEN.store(dlg as isize, Ordering::Relaxed);
        init(dlg, &mut *(lparam as *mut State));
        SetForegroundWindow(dlg);
        return 1; // focus the first control: the hotkey field
    }
    if msg == WM_DESTROY {
        OPEN.store(0, Ordering::Relaxed);
        return 0;
    }
    let state = GetWindowLongPtrW(dlg, GWLP_USERDATA) as *mut State;
    if msg != WM_COMMAND || state.is_null() {
        return 0;
    }
    match (wparam & 0xFFFF) as i32 {
        IDOK => save(dlg, &mut *state),
        IDCANCEL => {
            EndDialog(dlg, 0);
        }
        IDC_OPEN_DEBUG => open_folder(dlg, (*state).debug_dir, (*state).current.language),
        _ => return 0,
    }
    1
}

unsafe fn init(dlg: HWND, state: &mut State) {
    let lang = state.current.language;
    let text = |id: i32, s: String| SetDlgItemTextW(dlg, id, wide(&s).as_ptr());
    SetWindowTextW(dlg, wide(&lang.settings_title()).as_ptr());
    text(IDC_HOTKEY_LABEL, lang.hotkey_label());
    text(IDC_HOTKEY_HINT, lang.hotkey_hint());
    text(IDC_LANGUAGE_LABEL, lang.language_label());
    text(IDC_SHOW_TOAST, lang.show_toast_option());
    text(IDC_SAVE_DEBUG, lang.save_debug_option());
    text(IDC_OPEN_DEBUG, lang.open_folder());
    text(IDC_LOCATION, lang.settings_location(state.settings_path.display()));
    text(IDOK, lang.save());
    text(IDCANCEL, lang.cancel());

    // Each language is listed by its own name, in Lang::ALL order.
    for option in Lang::ALL {
        SendDlgItemMessageW(dlg, IDC_LANGUAGE, CB_ADDSTRING, 0, wide(option.name()).as_ptr() as LPARAM);
    }
    let selected = Lang::ALL.iter().position(|&l| l == lang).unwrap_or(0);
    SendDlgItemMessageW(dlg, IDC_LANGUAGE, CB_SETCURSEL, selected, 0);

    SendDlgItemMessageW(dlg, IDC_HOTKEY, HKM_SETHOTKEY, state.current.hotkey.to_field() as WPARAM, 0);
    state.initial_field = field_value(dlg);
    let check = |id: i32, on: bool| CheckDlgButton(dlg, id, if on { BST_CHECKED } else { BST_UNCHECKED });
    check(IDC_SHOW_TOAST, state.current.show_toast);
    check(IDC_SAVE_DEBUG, state.current.save_debug);
}

unsafe fn save(dlg: HWND, state: &mut State) {
    let lang = state.current.language;
    let field = field_value(dlg);
    let hotkey = if field == state.initial_field {
        Ok(state.current.hotkey.clone())
    } else if field & 0xFF == 0 {
        Err(lang.hotkey_empty())
    } else {
        Hotkey::from_field(field, lang)
    };
    let hotkey = match hotkey {
        Ok(h) => h,
        Err(e) => {
            win::message_box(dlg, &e, false);
            SetFocus(GetDlgItem(dlg, IDC_HOTKEY));
            return;
        }
    };
    let checked = |id: i32| IsDlgButtonChecked(dlg, id) == BST_CHECKED;
    let selected = SendDlgItemMessageW(dlg, IDC_LANGUAGE, CB_GETCURSEL, 0, 0);
    let language = usize::try_from(selected).ok().and_then(|i| Lang::ALL.get(i).copied()).unwrap_or(lang);
    let new = Settings { hotkey, language, show_toast: checked(IDC_SHOW_TOAST), save_debug: checked(IDC_SAVE_DEBUG) };
    match (state.apply)(&new) {
        Ok(()) => {
            state.saved = Some(new);
            EndDialog(dlg, 1);
        }
        Err(e) => win::message_box(dlg, &e, true),
    }
}

/// The hotkey field's value (see `Hotkey::to_field`).
unsafe fn field_value(dlg: HWND) -> u16 {
    SendDlgItemMessageW(dlg, IDC_HOTKEY, HKM_GETHOTKEY, 0, 0) as u16
}

unsafe fn open_folder(dlg: HWND, dir: &Path, lang: Lang) {
    let _ = std::fs::create_dir_all(dir);
    let path = wide(&dir.to_string_lossy());
    if ShellExecuteW(dlg, wide("open").as_ptr(), path.as_ptr(), null(), null(), SW_SHOWNORMAL) as isize <= 32 {
        win::message_box(dlg, &lang.open_failed(dir.display()), true);
    }
}
