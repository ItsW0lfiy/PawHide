use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use crate::folders;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub applications: Vec<String>,
    /// Canonical directories; protect only when an application reports an exact file path.
    pub folders: Vec<PathBuf>,
}

pub fn normalize_exe(name: &str) -> Result<String> {
    let name = name.trim();
    ensure!(!name.is_empty(), "application name cannot be empty");
    ensure!(
        !name.contains(['/', '\\', ':', '\0', '*', '?', '"', '<', '>', '|']),
        "use an executable name, not a path (example: PicView.exe)"
    );
    let value = name.to_ascii_lowercase();
    let value = if value.ends_with(".exe") { value } else { format!("{value}.exe") };
    // Prevent blanket injection into Explorer/system shell. Individual HWND
    // protection may be used experimentally with explicit --allow-shell.
    if value == "explorer.exe" {
        bail!("automatic explorer.exe protection is unsafe; use a dedicated window and --allow-shell only for manual tests");
    }
    Ok(value)
}

pub fn config_path() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from).map(|p| p.join(".config")))
        .context("cannot find LOCALAPPDATA (or HOME for tests)")?;
    Ok(base.join("PawHide").join("config.json"))
}

pub fn load() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() { return Ok(Config::default()); }
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut cfg: Config = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid configuration: {}", path.display()))?;
    let mut sorted = Vec::new();
    for entry in &cfg.applications {
        let name = normalize_exe(entry)?;
        if !sorted.contains(&name) { sorted.push(name); }
    }
    sorted.sort();
    cfg.applications = sorted;
    // Saved folders may be unavailable (removable/network drives). Keep them
    // configured; document notifications fail closed until they are reachable.
    folders::clean_saved_paths(&mut cfg.folders);
    Ok(cfg)
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    fs::create_dir_all(path.parent().expect("config path has parent"))?;
    let mut clean = cfg.clone();
    folders::clean_saved_paths(&mut clean.folders);
    let serialized = serde_json::to_vec_pretty(&clean)?;
    // A temp file prevents a partial JSON write from crashing a running watcher.
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&tmp, serialized)?;
    if let Err(err) = fs::rename(&tmp, &path) {
        // Windows cannot rename over an existing destination. A brief replacement
        // gap is preferable to clobbering partial JSON; watcher retains last config.
        if path.exists() {
            fs::remove_file(&path)?;
            fs::rename(&tmp, &path)?;
        } else { return Err(err.into()); }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_exe() {
        assert_eq!(normalize_exe("PicView.EXE").unwrap(), "picview.exe");
        assert_eq!(normalize_exe("floorp").unwrap(), "floorp.exe");
        assert_eq!(normalize_exe("NVIDIA Overlay.exe").unwrap(), "nvidia overlay.exe");
        assert!(normalize_exe("C:\\PicView.exe").is_err());
        assert!(normalize_exe("explorer.exe").is_err());
        assert!(normalize_exe("").is_err());
    }
    #[test]
    fn serde_defaults_and_rejects_unknown_fields() {
        assert!(serde_json::from_str::<Config>("{}").is_ok());
        assert!(serde_json::from_str::<Config>(r#"{"applications":[],"wrong":true}"#).is_err());
    }
}
