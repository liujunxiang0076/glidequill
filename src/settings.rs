use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::RwLock,
};

#[derive(Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    excluded: Vec<String>,
}
static EXCLUDED: RwLock<Vec<String>> = RwLock::new(Vec::new());

pub const IDLE_VALUES: [u32; 5] = [0, 500, 1000, 2000, 3000];
pub const RESULT_VALUES: [u32; 5] = [0, 250, 550, 1000, 2000];

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    #[serde(deserialize_with = "read_bindings")]
    pub bindings: [glidequill::Action; glidequill::GESTURES.len()],
    pub enabled: bool,
    pub pause_fullscreen: bool,
    pub dark: bool,
    pub idle_ms: u32,
    pub result_ms: u32,
}
// New gestures are appended so the two previous on-disk layouts keep their indices.
fn read_bindings<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<[glidequill::Action; glidequill::GESTURES.len()], D::Error> {
    let saved = Vec::<glidequill::Action>::deserialize(d)?;
    if ![glidequill::BASIC_COUNT, 16, glidequill::GESTURES.len()].contains(&saved.len()) {
        return Err(serde::de::Error::custom("绑定数量与支持的配置版本不符"));
    }
    let mut bindings = glidequill::DEFAULT_BINDINGS;
    bindings[..saved.len()].copy_from_slice(&saved);
    Ok(bindings)
}
const DEFAULTS: Preferences = Preferences {
    bindings: glidequill::DEFAULT_BINDINGS,
    enabled: false,
    pause_fullscreen: true,
    dark: false,
    idle_ms: 500,
    result_ms: 550,
};
impl Default for Preferences {
    fn default() -> Self {
        DEFAULTS
    }
}
pub type Bindings = [glidequill::Action; glidequill::GESTURES.len()];
pub type Overrides = [Option<glidequill::Action>; glidequill::GESTURES.len()];
pub type Applications = BTreeMap<String, Overrides>;

#[derive(Default, Clone, Deserialize, Serialize)]
struct Configuration {
    #[serde(flatten)]
    prefs: Preferences,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    applications: Applications,
}
static CONFIG: RwLock<Configuration> = RwLock::new(Configuration {
    prefs: DEFAULTS,
    applications: BTreeMap::new(),
});
pub fn preferences() -> Preferences {
    CONFIG.read().unwrap().prefs
}
pub fn applications() -> Applications {
    CONFIG.read().unwrap().applications.clone()
}
pub fn rule_changes(global: &Bindings, apps: &Applications) -> usize {
    let saved = CONFIG.read().unwrap();
    let mut count = global
        .iter()
        .zip(saved.prefs.bindings)
        .filter(|(a, b)| **a != *b)
        .count();
    for (name, bindings) in apps {
        let previous = saved.applications.get(name);
        count += usize::from(previous.is_none());
        count += bindings
            .iter()
            .zip(previous.unwrap_or(&[None; glidequill::GESTURES.len()]))
            .filter(|(a, b)| a != b)
            .count();
    }
    count
        + saved
            .applications
            .keys()
            .filter(|name| !apps.contains_key(*name))
            .count()
}
pub fn bindings_for(exe: &str) -> Bindings {
    let config = CONFIG.read().unwrap();
    let global = config.prefs.bindings;
    if config.applications.is_empty() {
        return global;
    }
    match config.applications.get(&exe.to_lowercase()) {
        Some(overrides) => std::array::from_fn(|i| overrides[i].unwrap_or(global[i])),
        None => global,
    }
}
pub fn excluded(exe: &str) -> bool {
    EXCLUDED
        .read()
        .unwrap()
        .iter()
        .any(|s| s.eq_ignore_ascii_case(exe))
}
pub fn exclusions() -> Vec<String> {
    EXCLUDED.read().unwrap().clone()
}
pub fn save_exclusions(names: &[String]) -> io::Result<()> {
    save_exclusions_in(&directory()?, names)
}
pub(super) fn save_exclusions_in(dir: &Path, names: &[String]) -> io::Result<()> {
    write_json(dir, "excluded.json", names)?;
    *EXCLUDED.write().unwrap() = names.to_vec();
    Ok(())
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
    let exclusions = match fs::read(dir.join("excluded.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            read::<Config>(&dir.join("rules.json"))?.excluded
        }
        Err(e) => return Err(e),
    };
    let config = read::<Configuration>(&dir.join("settings.json"))?;
    let prefs = config.prefs;
    if !IDLE_VALUES.contains(&prefs.idle_ms) || !RESULT_VALUES.contains(&prefs.result_ms) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "轨迹时间不在支持范围内",
        ));
    }
    *EXCLUDED.write().unwrap() = exclusions;
    *CONFIG.write().unwrap() = config;
    Ok(())
}
// All writes originate on the settings window's thread; readers never wait for disk I/O.
fn write_json<T: Serialize + ?Sized>(dir: &Path, name: &str, value: &T) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let temp = dir.join(format!("{name}.tmp"));
    fs::write(&temp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(temp, dir.join(name))
}
pub fn save(prefs: Preferences) -> io::Result<()> {
    persist(prefs, applications())
}
pub fn save_rules(bindings: Bindings, applications: Applications) -> io::Result<()> {
    persist(
        Preferences {
            bindings,
            ..preferences()
        },
        applications,
    )
}
fn persist(prefs: Preferences, applications: Applications) -> io::Result<()> {
    let config = Configuration {
        prefs,
        applications,
    };
    write_json(&directory()?, "settings.json", &config)?;
    *CONFIG.write().unwrap() = config;
    Ok(())
}
