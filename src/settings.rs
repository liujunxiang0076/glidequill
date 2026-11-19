use glidequill::library::{Library, Pack, SIGNS, Signature, signature};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{LazyLock, RwLock},
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
    #[serde(default = "Library::stock")]
    library: Library,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_rules: Option<Pack>,
    #[serde(skip)]
    shapes: BTreeMap<String, Signature>,
}
static CONFIG: LazyLock<RwLock<Configuration>> =
    LazyLock::new(|| RwLock::new(Configuration::default()));
pub fn pack() -> Pack {
    let config = CONFIG.read().unwrap();
    Pack {
        bindings: config.prefs.bindings,
        applications: config.applications.clone(),
        library: config.library.clone(),
    }
}
pub fn preferences() -> Preferences {
    CONFIG.read().unwrap().prefs
}
pub fn library_matches(lib: &Library) -> bool {
    CONFIG.read().unwrap().library == *lib
}
pub fn show_trail() -> bool {
    CONFIG.read().unwrap().library.tuning.trail
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
pub struct Resolved {
    pub wheel: [Option<glidequill::Action>; 2],
    pub bindings: Bindings,
    pub shapes: Vec<(Signature, glidequill::Action)>,
    pub tuning: glidequill::library::Tuning,
}
pub fn resolve(exe: &str) -> Resolved {
    let c = CONFIG.read().unwrap();
    let app = c.applications.get(&exe.to_lowercase());
    let bindings = std::array::from_fn(|i| {
        app.and_then(|a| a[i])
            .or_else(|| c.library.action(exe, SIGNS[i]))
            .unwrap_or(c.prefs.bindings[i])
    });
    let shapes = c
        .shapes
        .iter()
        .filter_map(|(sign, shape)| c.library.action(exe, sign).map(|a| (*shape, a)))
        .collect();
    Resolved {
        wheel: glidequill::library::WHEEL
            .map(|sign| c.library.action(exe, sign).filter(|a| a.keys().is_some())),
        bindings,
        shapes,
        tuning: c.library.tuning,
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
    let mut config = match fs::read(dir.join("settings.json")) {
        Ok(bytes) => serde_json::from_slice::<Configuration>(&bytes)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Configuration {
            library: Library::stock(),
            ..Default::default()
        },
        Err(e) => return Err(e),
    };
    config.prepare()?;
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
impl Configuration {
    fn prepare(&mut self) -> io::Result<()> {
        Pack {
            bindings: self.prefs.bindings,
            applications: self.applications.clone(),
            library: self.library.clone(),
        }
        .validate()
        .map_err(io::Error::other)?;
        self.shapes = self
            .library
            .shapes
            .iter()
            .filter(|(name, _)| {
                self.library
                    .rules
                    .iter()
                    .any(|r| r.sign.as_str() == name.as_str())
            })
            .filter_map(|(n, p)| signature(p).map(|s| (n.clone(), s)))
            .collect();
        Ok(())
    }
}
pub fn save(prefs: Preferences) -> io::Result<()> {
    let mut config = CONFIG.read().unwrap().clone();
    config.prefs = prefs;
    write_json(&directory()?, "settings.json", &config)?;
    *CONFIG.write().unwrap() = config;
    Ok(())
}
pub fn previous() -> io::Result<Pack> {
    if let Some(previous) = CONFIG.read().unwrap().previous_rules.as_ref() {
        previous.validate().map_err(io::Error::other)?;
        return Ok(previous.clone());
    }
    let bytes = fs::read(directory()?.join("previous-rules.json"))?;
    let pack: Pack = serde_json::from_slice(&bytes)?;
    pack.validate().map_err(io::Error::other)?;
    Ok(pack)
}
pub fn save_pack(next: &Pack) -> io::Result<()> {
    let mut config = CONFIG.read().unwrap().clone();
    config.prefs.bindings = next.bindings;
    config.applications = next.applications.clone();
    config.library = next.library.clone();
    config.prepare()?;
    let dir = directory()?;
    let previous = pack();
    config.previous_rules = Some(previous.clone());
    // The settings file commits both revisions together. The standalone copy
    // can still be opened if settings.json becomes unreadable.
    write_json(&dir, "previous-rules.json", &previous)?;
    write_json(&dir, "settings.json", &config)?;
    *CONFIG.write().unwrap() = config;
    Ok(())
}

pub fn recover_rules(next: &Pack) -> io::Result<PathBuf> {
    next.validate().map_err(io::Error::other)?;
    let dir = directory()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let archive = dir.join(format!("recovery-{stamp}"));
    fs::create_dir(&archive)?;
    for name in [
        "settings.json",
        "excluded.json",
        "rules.json",
        "previous-rules.json",
    ] {
        match fs::copy(dir.join(name), archive.join(name)) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    let exclusions = match fs::read(dir.join("excluded.json")) {
        Ok(v) => serde_json::from_slice::<Vec<String>>(&v).unwrap_or_default(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            read::<Config>(&dir.join("rules.json"))
                .unwrap_or_default()
                .excluded
        }
        Err(e) => return Err(e),
    };
    let mut config = Configuration {
        prefs: Preferences {
            bindings: next.bindings,
            ..Default::default()
        },
        applications: next.applications.clone(),
        library: next.library.clone(),
        ..Default::default()
    };
    config.prepare()?;
    write_json(&dir, "excluded.json", &exclusions)?;
    write_json(&dir, "settings.json", &config)?;
    load_from(&dir)?;
    Ok(archive)
}
