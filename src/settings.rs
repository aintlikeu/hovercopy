//! Settings: hotkey, toast and debug switches, stored as settings.ini.

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
    /// letters and digits need a modifier.
    pub fn from_parts(modifiers: u32, vk: u32) -> Result<Self, String> {
        let name = key_name(vk).ok_or("поддерживаются буквы A–Z, цифры 0–9 и F1–F24")?;
        let modifiers = modifiers & (MOD_ALT | MOD_CONTROL | MOD_SHIFT | MOD_WIN);
        if modifiers == 0 && !(VK_F1..VK_F1 + 24).contains(&vk) {
            return Err(format!("{name} нужно сочетать с Ctrl, Alt, Shift или Win, иначе клавиша перестанет печататься"));
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

    pub fn from_field(field: u16) -> Result<Self, String> {
        let flags = (field >> 8) as u32;
        let modifiers = FIELD_MODIFIERS.iter().filter(|(f, _)| flags & f != 0).fold(0, |acc, (_, m)| acc | m);
        Self::from_parts(modifiers, (field & 0xFF) as u32)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub hotkey: Hotkey,
    /// Show "Скопировано: …" after a copy. Failures are always shown.
    pub show_toast: bool,
    pub save_debug: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { hotkey: parse_hotkey("ctrl+alt+n").expect("valid default"), show_toast: true, save_debug: false }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut settings = Self::default();
        for (n, raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with([';', '#', '[']) {
                continue;
            }
            let at = |e: String| format!("строка {}: {e}", n + 1);
            let (key, value) = line.split_once('=').ok_or_else(|| at(format!("ожидается «параметр = значение»: {line}")))?;
            let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
            match key.as_str() {
                "hotkey" => settings.hotkey = parse_hotkey(value).map_err(at)?,
                "show_toast" => settings.show_toast = parse_bool(&key, value).map_err(at)?,
                "save_debug" => settings.save_debug = parse_bool(&key, value).map_err(at)?,
                other => return Err(at(format!("неизвестный параметр «{other}»"))),
            }
        }
        Ok(settings)
    }

    pub fn to_ini(&self) -> String {
        format!(
            "; HoverCopy — настройки. Удобнее менять их в программе: значок в трее → «Настройки».\r\n\
             ; Изменения, сделанные здесь вручную, применяются после перезапуска.\r\n\
             \r\n\
             ; Сочетание клавиш: ctrl / alt / shift / win + клавиша (a-z, 0-9, f1-f24).\r\n\
             ; Примеры: ctrl+alt+n, ctrl+shift+q, f8\r\n\
             hotkey = {}\r\n\
             \r\n\
             ; Показывать «Скопировано: …» после копирования (true / false).\r\n\
             show_toast = {}\r\n\
             \r\n\
             ; Сохранять снимки и результат распознавания для отладки (true / false).\r\n\
             save_debug = {}\r\n",
            self.hotkey.label.to_ascii_lowercase(),
            self.show_toast,
            self.save_debug
        )
    }
}

pub fn parse_hotkey(text: &str) -> Result<Hotkey, String> {
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
                return Err(format!("«{part}» указан дважды"));
            }
            modifiers |= modifier;
        } else if vk.is_some() {
            return Err(format!("в «{text}» больше одной обычной клавиши"));
        } else {
            vk = Some(parse_key(&lower).ok_or_else(|| format!("неизвестная клавиша «{part}» в «{text}»"))?);
        }
    }
    let vk = vk.ok_or_else(|| format!("в «{text}» нет обычной клавиши"))?;
    Hotkey::from_parts(modifiers, vk).map_err(|e| format!("«{text}»: {e}"))
}

fn parse_bool(key: &str, value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        _ => Err(format!("{key} должен быть true или false, а не «{value}»")),
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

    #[test]
    fn parses_hotkeys() {
        let k = parse_hotkey("ctrl+alt+n").unwrap();
        assert_eq!((k.modifiers, k.vk, k.label.as_str()), (MOD_CONTROL | MOD_ALT, 'N' as u32, "Ctrl+Alt+N"));
        let k = parse_hotkey(" Shift + Control + 7 ").unwrap();
        assert_eq!((k.modifiers, k.vk, k.label.as_str()), (MOD_CONTROL | MOD_SHIFT, '7' as u32, "Ctrl+Shift+7"));
        let k = parse_hotkey("F8").unwrap();
        assert_eq!((k.modifiers, k.vk, k.label.as_str()), (0, 0x77, "F8"));
        assert_eq!(parse_hotkey("win+f24").unwrap().vk, 0x87);
    }

    #[test]
    fn rejects_bad_hotkeys() {
        for bad in ["", "ctrl", "ctrl+alt", "n", "ctrl+ctrl+n", "ctrl+a+b", "ctrl+f25", "ctrl+f0", "ctrl+enter", "ctrl++n", "alt+ф"] {
            assert!(parse_hotkey(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn hotkey_from_parts_applies_the_same_rules() {
        assert_eq!(Hotkey::from_parts(MOD_SHIFT | MOD_ALT, 'Q' as u32).unwrap().label, "Alt+Shift+Q");
        assert_eq!(Hotkey::from_parts(0, 0x70).unwrap().label, "F1");
        assert!(Hotkey::from_parts(0, 'Q' as u32).is_err()); // a bare letter
        assert!(Hotkey::from_parts(MOD_CONTROL, 0x0D).is_err()); // Enter
        assert!(Hotkey::from_parts(MOD_CONTROL, 0x60).is_err()); // numpad 0
    }

    #[test]
    fn hotkey_field_swaps_shift_and_alt() {
        let hotkey = parse_hotkey("ctrl+shift+q").unwrap();
        assert_eq!(hotkey.to_field(), (0x1 | 0x2) << 8 | 'Q' as u16); // field Shift = 1, Ctrl = 2
        assert_eq!(Hotkey::from_field(hotkey.to_field()).unwrap(), hotkey);
        assert_eq!(Hotkey::from_field(0x4 << 8 | 0x70).unwrap().label, "Alt+F1"); // field Alt = 4
        assert_eq!(parse_hotkey("win+alt+n").unwrap().to_field(), 0x4 << 8 | 'N' as u16); // no Win
    }

    #[test]
    fn parses_ini() {
        assert_eq!(Settings::parse(&Settings::default().to_ini()).unwrap(), Settings::default());
        let s = Settings::parse("\u{feff}[main]\nhotkey = f9\n# comment\nSAVE_DEBUG = Yes\nshow_toast = 0\n").unwrap();
        assert_eq!((s.hotkey.label.as_str(), s.save_debug, s.show_toast), ("F9", true, false));
        assert!(Settings::parse("hotkye = f9").unwrap_err().contains("hotkye"));
        assert!(Settings::parse("save_debug = maybe").is_err());
        assert!(Settings::parse("show_toast = maybe").unwrap_err().contains("show_toast"));
        assert!(Settings::parse("hotkey f9").is_err());
    }

    #[test]
    fn ini_round_trip() {
        let s = Settings { hotkey: parse_hotkey("win+shift+f12").unwrap(), show_toast: false, save_debug: true };
        assert_eq!(Settings::parse(&s.to_ini()).unwrap(), s);
    }
}
