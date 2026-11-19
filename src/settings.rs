use crate::wide;
use glidequill::{Action, Config};
use std::sync::{Arc, LazyLock, RwLock};
use std::{
    io,
    path::{Path, PathBuf},
};
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

/// A gesture holds one immutable revision from button press through release.
/// The lock protects only the pointer swap, never disk I/O or Win32 calls.
static ACTIVE: LazyLock<RwLock<Arc<Config>>> =
    LazyLock::new(|| RwLock::new(Arc::new(Config::default())));
pub fn snapshot() -> Arc<Config> {
    Arc::clone(&ACTIVE.read().unwrap())
}
pub fn initialize(config: Config) {
    *ACTIVE.write().unwrap() = Arc::new(config);
}
fn validate(config: &Config) -> io::Result<()> {
    if config.global.contains(&Action::Inherit) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "通用规则不能继承自身",
        ));
    }
    Ok(())
}
pub fn apply(config: Config) -> io::Result<()> {
    apply_at(config, &path()?)
}
fn apply_at(config: Config, file: &Path) -> io::Result<()> {
    validate(&config)?;
    save(&config, file)?;
    initialize(config);
    Ok(())
}
fn path() -> io::Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or(io::ErrorKind::NotFound)?)
            .join("GlideQuill/rules.json"),
    )
}
pub fn load() -> io::Result<Config> {
    match std::fs::read(path()?) {
        Ok(b) => {
            let rules = serde_json::from_slice(&b)?;
            validate(&rules)?;
            Ok(rules)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e),
    }
}
fn save(c: &Config, file: &Path) -> io::Result<()> {
    std::fs::create_dir_all(file.parent().unwrap())?;
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(c)?)?;
    if unsafe {
        MoveFileExW(
            wide(&tmp.to_string_lossy()).as_ptr(),
            wide(&file.to_string_lossy()).as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publish_only_after_success_and_preserve_in_flight_revision() {
        let dir = std::env::temp_dir().join(format!(
            "glidequill-rule-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let file = dir.join("rules.json");
        let before = snapshot();
        let mut changed = (*before).clone();
        changed.global[0] = Action::End;
        apply_at(changed.clone(), &file).unwrap();
        assert_eq!(before.global[0], Action::Back);
        let committed = snapshot();
        assert_eq!(committed.global[0], Action::End);
        changed.global[0] = Action::Inherit;
        assert!(apply_at(changed.clone(), &file).is_err());
        changed.global[0] = Action::Home;
        assert!(apply_at(changed, &file.join("cannot-create-here")).is_err());
        assert!(Arc::ptr_eq(&snapshot(), &committed));
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
