use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

#[derive(Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    excluded: Vec<String>,
}
static CONFIG: OnceLock<Config> = OnceLock::new();

pub const IDLE_VALUES: [u32; 5] = [0, 500, 1000, 2000, 3000];
pub const RESULT_VALUES: [u32; 5] = [0, 250, 550, 1000, 2000];

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    #[serde(deserialize_with = "read_bindings")]
    pub bindings: [glidequill::Action; glidequill::GESTURES.len()],
    pub enabled: bool,
    pub dark: bool,
    pub idle_ms: u32,
    pub result_ms: u32,
}
// Preserve bindings saved before the two-leg gestures were introduced.
fn read_bindings<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<[glidequill::Action; glidequill::GESTURES.len()], D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Saved {
        Current([glidequill::Action; glidequill::GESTURES.len()]),
        Basic([glidequill::Action; glidequill::BASIC_COUNT]),
    }
    Ok(match Saved::deserialize(d)? {
        Saved::Current(all) => all,
        Saved::Basic(basic) => {
            let mut all = glidequill::DEFAULT_BINDINGS;
            all[..basic.len()].copy_from_slice(&basic);
            all
        }
    })
}
const DEFAULTS: Preferences = Preferences {
    bindings: glidequill::DEFAULT_BINDINGS,
    enabled: false,
    dark: false,
    idle_ms: 500,
    result_ms: 550,
};
impl Default for Preferences {
    fn default() -> Self {
        DEFAULTS
    }
}
static PREFS: RwLock<Preferences> = RwLock::new(DEFAULTS);
pub fn preferences() -> Preferences {
    *PREFS.read().unwrap()
}
pub fn excluded(exe: &str) -> bool {
    CONFIG
        .get()
        .is_some_and(|c| c.excluded.iter().any(|s| s.eq_ignore_ascii_case(exe)))
}
fn directory() -> io::Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or(io::ErrorKind::NotFound)?)
            .join("GlideQuill"),
    )
}
fn read<T: DeserializeOwned + Default>(path: &Path) -> io::Result<T> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e),
    }
}
pub fn load() -> io::Result<()> {
    load_from(&directory()?)
}
pub(super) fn load_from(dir: &Path) -> io::Result<()> {
    let exclusions = read::<Config>(&dir.join("rules.json"))?;
    let prefs = read::<Preferences>(&dir.join("settings.json"))?;
    if !IDLE_VALUES.contains(&prefs.idle_ms) || !RESULT_VALUES.contains(&prefs.result_ms) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "轨迹时间不在支持范围内",
        ));
    }
    let _ = CONFIG.set(exclusions);
    *PREFS.write().unwrap() = prefs;
    Ok(())
}
// All writes originate on the settings window's thread; readers never wait for disk I/O.
pub fn save(prefs: Preferences) -> io::Result<()> {
    let dir = directory()?;
    fs::create_dir_all(&dir)?;
    let temp = dir.join("settings.json.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(&prefs)?)?;
    fs::rename(temp, dir.join("settings.json"))?;
    *PREFS.write().unwrap() = prefs;
    Ok(())
}
