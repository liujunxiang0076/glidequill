use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Chord {
    modifiers: u8,
    key: u16,
}
const MODIFIERS: [(&str, u16); 4] = [
    ("Ctrl", 0x11),
    ("Alt", 0x12),
    ("Shift", 0x10),
    ("Win", 0x5b),
];
const NAMED: &[(&str, u16)] = &[
    ("Back", 0xa6),
    ("Forward", 0xa7),
    ("Home", 0x24),
    ("End", 0x23),
    ("PgUp", 0x21),
    ("PgDn", 0x22),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("Esc", 0x1b),
    ("Del", 0x2e),
    ("Insert", 0x2d),
    ("Backspace", 8),
    ("Tab", 9),
    ("Enter", 13),
    ("Space", 32),
];
impl Chord {
    pub fn key(self) -> u16 {
        self.key
    }
    pub fn modifiers(self) -> impl Iterator<Item = u16> {
        MODIFIERS
            .into_iter()
            .enumerate()
            .filter_map(move |(i, (_, key))| (self.modifiers & (1 << i) != 0).then_some(key))
    }
}
impl std::str::FromStr for Chord {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.trim().split('+').map(str::trim).peekable();
        let mut modifiers = 0;
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                let i = MODIFIERS
                    .iter()
                    .position(|(name, _)| name.eq_ignore_ascii_case(part))
                    .ok_or("修饰键支持 Ctrl、Alt、Shift 和 Win")?;
                if modifiers & (1 << i) != 0 {
                    return Err("修饰键重复".into());
                }
                modifiers |= 1 << i;
            } else {
                let key = part.to_ascii_uppercase();
                let vk = if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() {
                    Some(key.as_bytes()[0] as u16)
                } else if let Some(n) = key
                    .strip_prefix('F')
                    .and_then(|n| n.parse::<u16>().ok())
                    .filter(|n| (1..=24).contains(n))
                {
                    Some(0x6f + n)
                } else {
                    NAMED
                        .iter()
                        .find(|(name, _)| name.eq_ignore_ascii_case(part))
                        .map(|v| v.1)
                };
                return Ok(Self {
                    modifiers,
                    key: vk.ok_or("请输入一个字母、数字、F1–F24 或导航键，例如 Ctrl+Shift+T")?,
                });
            }
        }
        Err("请输入快捷键".into())
    }
}
impl TryFrom<String> for Chord {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        value.parse()
    }
}
impl From<Chord> for String {
    fn from(value: Chord) -> Self {
        value.to_string()
    }
}
impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (name, _)) in MODIFIERS.iter().enumerate() {
            if self.modifiers & (1 << i) != 0 {
                write!(f, "{name}+")?;
            }
        }
        if let Some((name, _)) = NAMED.iter().find(|v| v.1 == self.key) {
            f.write_str(name)
        } else if (0x70..=0x87).contains(&self.key) {
            write!(f, "F{}", self.key - 0x6f)
        } else {
            write!(f, "{}", char::from_u32(self.key.into()).unwrap_or('?'))
        }
    }
}
