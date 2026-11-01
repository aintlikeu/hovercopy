//! Settings: hotkey, language, toast and debug switches, stored as settings.ini.

use crate::i18n::Lang;

// RegisterHotKey modifier flags.
pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

const VK_F1: u32 = 0x70;

// Modifier flags of the Windows hotkey field (HKM_GETHOTKEY): Shift and Alt are swapped
// compared to RegisterHotKey, and there is no Win.
const FIELD_MODIFIERS: [(u32, u32); 3] = [(0x1, MOD_SHIFT), (0x2, MOD_CONTROL), (0x4, MOD_ALT)];

#[derive(Clone, Debug, PartialEq)]
pub struct Hotkey {
    pub modifiers: u32,
    pub vk: u32,
    /// "Ctrl+Alt+N"
    pub label: String,
}

impl Hotkey {
    /// Checks a modifier set (MOD_*) and virtual-key code: letters, digits and F1–F24, where
    /// letters and digits need a modifier. Errors are in `lang`.
    pub fn from_parts(modifiers: u32, vk: u32, lang: Lang) -> Result<Self, String> {
        let name = key_name(vk).ok_or_else(|| lang.key_unsupported())?;
        let modifiers = modifiers & (MOD_ALT | MOD_CONTROL | MOD_SHIFT | MOD_WIN);
        if modifiers == 0 && !(VK_F1..VK_F1 + 24).contains(&vk) {
            return Err(lang.key_needs_modifier(&name));
        }
        let names = [(MOD_CONTROL, "Ctrl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift"), (MOD_WIN, "Win")];
        let mut label: Vec<&str> = names.iter().filter(|(m, _)| modifiers & m != 0).map(|(_, n)| *n).collect();
        label.push(&name);
        Ok(Self { modifiers, vk, label: label.join("+") })
    }

    /// Value for the Windows hotkey field: the key in the low byte, its modifier flags in the
    /// high byte. Win cannot be shown there and is left out.
    pub fn to_field(&self) -> u16 {
        let flags = FIELD_MODIFIERS.iter().filter(|(_, m)| self.modifiers & m != 0).fold(0, |acc, (f, _)| acc | f);
        (flags << 8 | self.vk & 0xFF) as u16
    }

    pub fn from_field(field: u16, lang: Lang) -> Result<Self, String> {
        let flags = (field >> 8) as u32;
        let modifiers = FIELD_MODIFIERS.iter().filter(|(f, _)| flags & f != 0).fold(0, |acc, (_, m)| acc | m);
        Self::from_parts(modifiers, (field & 0xFF) as u32, lang)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub hotkey: Hotkey,
    pub language: Lang,
    /// Show "Copied: …" after a copy. Failures are always shown.
    pub show_toast: bool,
    pub save_debug: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: parse_hotkey("ctrl+alt+n", Lang::En).expect("valid default"),
            language: Lang::default(),
            show_toast: true,
            save_debug: false,
        }
    }
}

impl Settings {
    /// Errors are in the language the file sets, so that they match the rest of the interface.
    pub fn parse(text: &str) -> Result<Self, String> {
        let lang = Self::language_in(text);
        let mut settings = Self::default();
        for (n, line, entry) in entries(text) {
            let at = |e: String| lang.at_line(n, &e);
            let (key, value) = entry.ok_or_else(|| at(lang.expected_key_value(line)))?;
            match key.as_str() {
                "hotkey" => settings.hotkey = parse_hotkey(value, lang).map_err(at)?,
                "language" => settings.language = Lang::from_code(value).ok_or_else(|| at(lang.expected_language(value)))?,
                "show_toast" => settings.show_toast = parse_bool(&key, value, lang).map_err(at)?,
                "save_debug" => settings.save_debug = parse_bool(&key, value, lang).map_err(at)?,
                other => return Err(at(lang.unknown_setting(other))),
            }
        }
        Ok(settings)
    }

    /// The language a settings file asks for, even if other lines are broken.
    pub fn language_in(text: &str) -> Lang {
        entries(text)
            .filter_map(|(_, _, entry)| entry)
            .filter(|(key, _)| key == "language")
            .find_map(|(_, value)| Lang::from_code(value))
            .unwrap_or_default()
    }

    /// The file, with comments in the settings' own language.
    pub fn to_ini(&self) -> String {
        let [header, hotkey, examples, language, toast, debug] = self.language.ini_comments();
        format!(
            "{header}\r\n\r\n{hotkey}\r\n{examples}\r\nhotkey = {}\r\n\r\n{language}\r\nlanguage = {}\r\n\r\n\
             {toast}\r\nshow_toast = {}\r\n\r\n{debug}\r\nsave_debug = {}\r\n",
            self.hotkey.label.to_ascii_lowercase(),
            self.language.code(),
            self.show_toast,
            self.save_debug
        )
    }
}

/// Meaningful lines of an ini text: (line number, line, Some((lowercase key, value)) or None
/// if there is no "=").
fn entries(text: &str) -> impl Iterator<Item = (usize, &str, Option<(String, &str)>)> {
    text.trim_start_matches('\u{feff}').lines().enumerate().filter_map(|(n, raw)| {
        let line = raw.trim();
        if line.is_empty() || line.starts_with([';', '#', '[']) {
            return None;
        }
        let entry = line.split_once('=').map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim()));
        Some((n + 1, line, entry))
    })
}

pub fn parse_hotkey(text: &str, lang: Lang) -> Result<Hotkey, String> {
    let mut modifiers = 0;
    let mut vk = None;
    for part in text.split('+').map(str::trim) {
        let lower = part.to_ascii_lowercase();
        let modifier = match lower.as_str() {
            "ctrl" | "control" => MOD_CONTROL,
            "alt" => MOD_ALT,
            "shift" => MOD_SHIFT,
            "win" => MOD_WIN,
            _ => 0,
        };
        if modifier != 0 {
            if modifiers & modifier != 0 {
                return Err(lang.modifier_twice(part));
            }
            modifiers |= modifier;
        } else if vk.is_some() {
            return Err(lang.several_keys(text));
        } else {
            vk = Some(parse_key(&lower).ok_or_else(|| lang.unknown_key(part, text))?);
        }
    }
    let vk = vk.ok_or_else(|| lang.no_key(text))?;
    Hotkey::from_parts(modifiers, vk, lang).map_err(|e| lang.in_hotkey(text, &e))
}

fn parse_bool(key: &str, value: &str, lang: Lang) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        _ => Err(lang.expected_bool(key, value)),
    }
}

/// Virtual-key code of a key name: "a"–"z", "0"–"9", "f1"–"f24".
fn parse_key(key: &str) -> Option<u32> {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c @ ('a'..='z' | '0'..='9')), None) => Some(c.to_ascii_uppercase() as u32),
        (Some('f'), Some(_)) => key[1..].parse::<u32>().ok().filter(|n| (1..=24).contains(n)).map(|n| VK_F1 + n - 1),
        _ => None,
    }
}

fn key_name(vk: u32) -> Option<String> {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).map(String::from),
        _ if (VK_F1..VK_F1 + 24).contains(&vk) => Some(format!("F{}", vk - VK_F1 + 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hotkey(text: &str) -> Hotkey {
        parse_hotkey(text, Lang::En).unwrap()
    }

    #[test]
    fn parses_hotkeys() {
        let k = hotkey("ctrl+alt+n");
        assert_eq!((k.modifiers, k.vk, k.label.as_str()), (MOD_CONTROL | MOD_ALT, 'N' as u32, "Ctrl+Alt+N"));
        let k = hotkey(" Shift + Control + 7 ");
        assert_eq!((k.modifiers, k.vk, k.label.as_str()), (MOD_CONTROL | MOD_SHIFT, '7' as u32, "Ctrl+Shift+7"));
        let k = hotkey("F8");
        assert_eq!((k.modifiers, k.vk, k.label.as_str()), (0, 0x77, "F8"));
        assert_eq!(hotkey("win+f24").vk, 0x87);
    }

    #[test]
    fn rejects_bad_hotkeys() {
        for bad in ["", "ctrl", "ctrl+alt", "n", "ctrl+ctrl+n", "ctrl+a+b", "ctrl+f25", "ctrl+f0", "ctrl+enter", "ctrl++n", "alt+ф"] {
            assert!(parse_hotkey(bad, Lang::En).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn hotkey_from_parts_applies_the_same_rules() {
        assert_eq!(Hotkey::from_parts(MOD_SHIFT | MOD_ALT, 'Q' as u32, Lang::En).unwrap().label, "Alt+Shift+Q");
        assert_eq!(Hotkey::from_parts(0, 0x70, Lang::En).unwrap().label, "F1");
        assert!(Hotkey::from_parts(0, 'Q' as u32, Lang::En).is_err()); // a bare letter
        assert!(Hotkey::from_parts(MOD_CONTROL, 0x0D, Lang::En).is_err()); // Enter
        assert!(Hotkey::from_parts(MOD_CONTROL, 0x60, Lang::En).is_err()); // numpad 0
        // Errors follow the language.
        assert!(Hotkey::from_parts(0, 'Q' as u32, Lang::En).unwrap_err().contains("needs Ctrl"));
        assert!(Hotkey::from_parts(0, 'Q' as u32, Lang::Ru).unwrap_err().contains("нужно сочетать"));
    }

    #[test]
    fn hotkey_field_swaps_shift_and_alt() {
        let k = hotkey("ctrl+shift+q");
        assert_eq!(k.to_field(), (0x1 | 0x2) << 8 | 'Q' as u16); // field Shift = 1, Ctrl = 2
        assert_eq!(Hotkey::from_field(k.to_field(), Lang::En).unwrap(), k);
        assert_eq!(Hotkey::from_field(0x4 << 8 | 0x70, Lang::En).unwrap().label, "Alt+F1"); // field Alt = 4
        assert_eq!(hotkey("win+alt+n").to_field(), 0x4 << 8 | 'N' as u16); // no Win
    }

    #[test]
    fn parses_ini() {
        assert_eq!(Settings::parse(&Settings::default().to_ini()).unwrap(), Settings::default());
        let s = Settings::parse("\u{feff}[main]\nhotkey = f9\n# comment\nSAVE_DEBUG = Yes\nshow_toast = 0\nlanguage = RU\n").unwrap();
        assert_eq!((s.hotkey.label.as_str(), s.save_debug, s.show_toast, s.language), ("F9", true, false, Lang::Ru));
        assert!(Settings::parse("hotkye = f9").unwrap_err().contains("hotkye"));
        assert!(Settings::parse("save_debug = maybe").is_err());
        assert!(Settings::parse("show_toast = maybe").unwrap_err().contains("show_toast"));
        assert!(Settings::parse("language = de").unwrap_err().contains("en or ru"));
        assert!(Settings::parse("hotkey f9").is_err());
    }

    #[test]
    fn errors_and_comments_follow_the_files_language() {
        assert!(Settings::parse("hotkye = f9").unwrap_err().starts_with("line 1:"));
        let broken = "language = ru\nhotkey = ctrl+ф\n";
        assert!(Settings::parse(broken).unwrap_err().starts_with("строка 2:"));
        assert_eq!(Settings::language_in(broken), Lang::Ru); // usable even though parse fails
        assert_eq!(Settings::language_in("hotkey = f9"), Lang::En);
        let ru = Settings { language: Lang::Ru, ..Settings::default() };
        assert!(ru.to_ini().contains("Сочетание клавиш") && Settings::default().to_ini().contains("Shortcut"));
    }

    #[test]
    fn ini_round_trip() {
        let s = Settings { hotkey: hotkey("win+shift+f12"), language: Lang::Ru, show_toast: false, save_debug: true };
        assert_eq!(Settings::parse(&s.to_ini()).unwrap(), s);
    }
}
