// This milestone only reads exclusions. Older mappings remain intact on disk.
use serde::Deserialize;
use std::{io, path::PathBuf, sync::OnceLock};

#[derive(Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    excluded: Vec<String>,
}
static CONFIG: OnceLock<Config> = OnceLock::new();
pub fn initialize(config: Config) {
    let _ = CONFIG.set(config);
}
pub fn excluded(exe: &str) -> bool {
    CONFIG
        .get()
        .is_some_and(|c| c.excluded.iter().any(|name| name.eq_ignore_ascii_case(exe)))
}
pub fn load() -> io::Result<Config> {
    let path = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or(io::ErrorKind::NotFound)?)
        .join("GlideQuill/rules.json");
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e),
    }
}
