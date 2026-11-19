use super::*;
use std::sync::{Arc, LazyLock, RwLock};

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
    validate(&config)?;
    save(&config)?;
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
fn save(c: &Config) -> io::Result<()> {
    let file = path()?;
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
