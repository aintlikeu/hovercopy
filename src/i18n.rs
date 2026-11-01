//! Interface texts in English and Russian. Each text is a method with both languages side by
//! side, so a missing translation does not compile.

use std::fmt::Display;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    En,
    Ru,
}

impl Lang {
    /// In the order of the language list in the settings window.
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ru];

    /// Code in settings.ini.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Self::ALL.into_iter().find(|l| l.code().eq_ignore_ascii_case(code.trim()))
    }

    /// Name in the language itself, for the language list.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ru => "Русский",
        }
    }

    fn pick(self, en: &str, ru: &str) -> String {
        match self {
            Lang::En => en,
            Lang::Ru => ru,
        }
        .to_owned()
    }

    // Toasts and messages.

    pub fn copied(self, word: &str) -> String {
        match self {
            Lang::En => format!("Copied: {word}"),
            Lang::Ru => format!("Скопировано: {word}"),
        }
    }

    pub fn not_copied(self, word: &str, error: impl Display) -> String {
        match self {
            Lang::En => format!("{word} — not copied: {error}"),
            Lang::Ru => format!("{word} — не скопировано: {error}"),
        }
    }

    pub fn text_not_found(self) -> String {
        self.pick("No text found", "Текст не найден")
    }

    pub fn error(self, error: impl Display) -> String {
        match self {
            Lang::En => format!("Error: {error}"),
            Lang::Ru => format!("Ошибка: {error}"),
        }
    }

    pub fn capture_failed(self, error: impl Display) -> String {
        match self {
            Lang::En => format!("Could not capture the screen: {error}"),
            Lang::Ru => format!("Не удалось снять экран: {error}"),
        }
    }

    pub fn recognition_stopped(self) -> String {
        self.pick("Recognition has stopped — restart HoverCopy", "Распознавание остановилось — перезапустите HoverCopy")
    }

    pub fn models_failed(self, error: impl Display) -> String {
        match self {
            Lang::En => format!("OCR models failed to load: {error}"),
            Lang::Ru => format!("не загрузились модели OCR: {error}"),
        }
    }

    pub fn recognition_crashed(self) -> String {
        self.pick("Recognition failed", "Сбой распознавания")
    }

    pub fn clipboard_busy(self) -> String {
        self.pick("the clipboard is held by another program", "буфер обмена занят другой программой")
    }

    pub fn already_running(self) -> String {
        self.pick(
            "HoverCopy is already running — its icon is in the notification area, next to the clock.",
            "HoverCopy уже запущен — его значок в трее, рядом с часами.",
        )
    }

    pub fn window_failed(self, code: u32) -> String {
        match self {
            Lang::En => format!("Could not create a window (error {code})."),
            Lang::Ru => format!("Не удалось создать окно (ошибка {code})."),
        }
    }

    pub fn settings_file_error(self, path: impl Display, error: impl Display) -> String {
        match self {
            Lang::En => format!("Error in {path}:\n{error}\n\nDefault settings are used for now."),
            Lang::Ru => format!("Ошибка в {path}:\n{error}\n\nПока действуют настройки по умолчанию."),
        }
    }

    pub fn hotkey_taken(self, label: &str) -> String {
        match self {
            Lang::En => format!("The shortcut {label} is already used by another program. Choose another one."),
            Lang::Ru => format!("Сочетание {label} уже занято другой программой. Выберите другое."),
        }
    }

    pub fn save_failed(self, path: impl Display, error: impl Display) -> String {
        match self {
            Lang::En => format!("Could not save {path}:\n{error}"),
            Lang::Ru => format!("Не удалось сохранить {path}:\n{error}"),
        }
    }

    pub fn open_failed(self, path: impl Display) -> String {
        match self {
            Lang::En => format!("Could not open {path}"),
            Lang::Ru => format!("Не удалось открыть {path}"),
        }
    }

    // Tray.

    pub fn tray_tip(self, hotkey: &str) -> String {
        match self {
            Lang::En => format!("HoverCopy — copy the text under the cursor: {hotkey}"),
            Lang::Ru => format!("HoverCopy — копировать текст под курсором: {hotkey}"),
        }
    }

    pub fn tray_tip_no_hotkey(self) -> String {
        self.pick("HoverCopy — the shortcut does not work, open Settings", "HoverCopy — сочетание клавиш не работает, откройте «Настройки»")
    }

    pub fn menu_settings(self) -> String {
        self.pick("Settings…", "Настройки…")
    }

    pub fn menu_exit(self) -> String {
        self.pick("Exit", "Выход")
    }

    // Settings window.

    pub fn settings_title(self) -> String {
        self.pick("HoverCopy Settings", "HoverCopy — настройки")
    }

    pub fn hotkey_label(self) -> String {
        self.pick("Shortcut:", "Сочетание клавиш:")
    }

    pub fn hotkey_hint(self) -> String {
        self.pick(
            "Click the field and press the shortcut. Use Ctrl, Alt or Shift; F1–F24 also work alone.",
            "Щёлкните в поле и нажмите сочетание. Нужен Ctrl, Alt или Shift; F1–F24 можно без них.",
        )
    }

    pub fn hotkey_empty(self) -> String {
        self.pick("Set a shortcut: click the field and press it.", "Задайте сочетание клавиш: щёлкните в поле и нажмите его.")
    }

    pub fn language_label(self) -> String {
        self.pick("Language:", "Язык:")
    }

    pub fn show_toast_option(self) -> String {
        self.pick("Show “Copied: …” after copying", "Показывать «Скопировано: …» после копирования")
    }

    pub fn save_debug_option(self) -> String {
        self.pick("Save captures for debugging", "Сохранять снимки для отладки")
    }

    pub fn open_folder(self) -> String {
        self.pick("Open folder", "Открыть папку")
    }

    pub fn settings_location(self, path: impl Display) -> String {
        match self {
            Lang::En => format!("Settings file: {path}"),
            Lang::Ru => format!("Файл настроек: {path}"),
        }
    }

    pub fn save(self) -> String {
        self.pick("Save", "Сохранить")
    }

    pub fn cancel(self) -> String {
        self.pick("Cancel", "Отмена")
    }

    // Settings validation.

    pub fn key_unsupported(self) -> String {
        self.pick("only letters A–Z, digits 0–9 and F1–F24 are supported", "поддерживаются буквы A–Z, цифры 0–9 и F1–F24")
    }

    pub fn key_needs_modifier(self, key: &str) -> String {
        match self {
            Lang::En => format!("{key} needs Ctrl, Alt, Shift or Win, otherwise the key would stop typing"),
            Lang::Ru => format!("{key} нужно сочетать с Ctrl, Alt, Shift или Win, иначе клавиша перестанет печататься"),
        }
    }

    pub fn modifier_twice(self, part: &str) -> String {
        match self {
            Lang::En => format!("“{part}” is given twice"),
            Lang::Ru => format!("«{part}» указан дважды"),
        }
    }

    pub fn several_keys(self, text: &str) -> String {
        match self {
            Lang::En => format!("“{text}” has more than one ordinary key"),
            Lang::Ru => format!("в «{text}» больше одной обычной клавиши"),
        }
    }

    pub fn unknown_key(self, part: &str, text: &str) -> String {
        match self {
            Lang::En => format!("unknown key “{part}” in “{text}”"),
            Lang::Ru => format!("неизвестная клавиша «{part}» в «{text}»"),
        }
    }

    pub fn no_key(self, text: &str) -> String {
        match self {
            Lang::En => format!("“{text}” has no ordinary key"),
            Lang::Ru => format!("в «{text}» нет обычной клавиши"),
        }
    }

    pub fn in_hotkey(self, text: &str, error: &str) -> String {
        match self {
            Lang::En => format!("“{text}”: {error}"),
            Lang::Ru => format!("«{text}»: {error}"),
        }
    }

    pub fn at_line(self, line: usize, error: &str) -> String {
        match self {
            Lang::En => format!("line {line}: {error}"),
            Lang::Ru => format!("строка {line}: {error}"),
        }
    }

    pub fn expected_key_value(self, line: &str) -> String {
        match self {
            Lang::En => format!("expected “setting = value”: {line}"),
            Lang::Ru => format!("ожидается «параметр = значение»: {line}"),
        }
    }

    pub fn expected_bool(self, key: &str, value: &str) -> String {
        match self {
            Lang::En => format!("{key} must be true or false, not “{value}”"),
            Lang::Ru => format!("{key} должен быть true или false, а не «{value}»"),
        }
    }

    pub fn expected_language(self, value: &str) -> String {
        match self {
            Lang::En => format!("language must be en or ru, not “{value}”"),
            Lang::Ru => format!("language должен быть en или ru, а не «{value}»"),
        }
    }

    pub fn unknown_setting(self, key: &str) -> String {
        match self {
            Lang::En => format!("unknown setting “{key}”"),
            Lang::Ru => format!("неизвестный параметр «{key}»"),
        }
    }

    /// Comment lines of settings.ini, in file order: header, two hotkey lines, language,
    /// toast, debug.
    pub fn ini_comments(self) -> [&'static str; 6] {
        match self {
            Lang::En => [
                "; HoverCopy settings. It is easier to change them in the program: tray icon → Settings.\r\n\
                 ; Changes made here by hand apply after a restart.",
                "; Shortcut: ctrl / alt / shift / win + key (a-z, 0-9, f1-f24).",
                "; Examples: ctrl+alt+n, ctrl+shift+q, f8",
                "; Interface language: en or ru.",
                "; Show “Copied: …” after copying (true / false).",
                "; Save captures and recognition results for debugging (true / false).",
            ],
            Lang::Ru => [
                "; HoverCopy — настройки. Удобнее менять их в программе: значок в трее → «Настройки».\r\n\
                 ; Изменения, сделанные здесь вручную, применяются после перезапуска.",
                "; Сочетание клавиш: ctrl / alt / shift / win + клавиша (a-z, 0-9, f1-f24).",
                "; Примеры: ctrl+alt+n, ctrl+shift+q, f8",
                "; Язык интерфейса: en или ru.",
                "; Показывать «Скопировано: …» после копирования (true / false).",
                "; Сохранять снимки и результат распознавания для отладки (true / false).",
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::from_code(" RU "), Some(Lang::Ru));
        assert_eq!(Lang::from_code("de"), None);
        assert_eq!(Lang::default(), Lang::En);
    }

    #[test]
    fn texts_carry_their_arguments() {
        for lang in Lang::ALL {
            assert!(lang.copied("w0rd").contains("w0rd"));
            assert!(lang.hotkey_taken("Ctrl+Q").contains("Ctrl+Q"));
            assert!(lang.at_line(7, "oops").contains('7') && lang.at_line(7, "oops").contains("oops"));
        }
        assert_ne!(Lang::En.copied("x"), Lang::Ru.copied("x"));
    }
}
