use crate::folders;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub applications: Vec<String>,
    /// Canonical directories; protect only when an application reports an exact file path.
    pub folders: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Preferences::is_default")]
    pub preferences: Preferences,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub show_system_windows: bool,
    pub capture_cursor: bool,
    pub capture_native_resolution: bool,
    pub capture_thumbnail: bool,
}

impl Preferences {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

pub fn normalize_exe(name: &str) -> Result<String> {
    let name = name.trim();
    ensure!(!name.is_empty(), "application name cannot be empty");
    ensure!(
        !name.contains(['/', '\\', ':', '\0', '*', '?', '"', '<', '>', '|']),
        "use an executable name, not a path (example: PicView.exe)"
    );
    let value = name.to_ascii_lowercase();
    let value = if value.ends_with(".exe") {
        value
    } else {
        format!("{value}.exe")
    };
    // Prevent blanket injection into Explorer/system shell. Individual HWND
    // protection may be used experimentally with explicit --allow-shell.
    if value == "explorer.exe" {
        bail!(
            "automatic explorer.exe protection is unsafe; use a dedicated window and --allow-shell only for manual tests"
        );
    }
    Ok(value)
}

pub fn config_path() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|p| p.join(".config"))
        })
        .context("cannot find LOCALAPPDATA (or HOME for tests)")?;
    Ok(base.join("PawHide").join("config.json"))
}

pub fn load() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut cfg: Config = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid configuration: {}", path.display()))?;
    let mut sorted = Vec::new();
    for entry in &cfg.applications {
        let name = normalize_exe(entry)?;
        if !sorted.contains(&name) {
            sorted.push(name);
        }
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
    save_at(&path, cfg)
}

fn save_at(path: &std::path::Path, cfg: &Config) -> Result<()> {
    fs::create_dir_all(path.parent().expect("config path has parent"))?;
    let mut clean = cfg.clone();
    folders::clean_saved_paths(&mut clean.folders);
    let serialized = serde_json::to_vec_pretty(&clean)?;
    // A temp file prevents a partial JSON write from crashing a running watcher.
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&tmp, serialized)?;
    if let Err(err) = fs::rename(&tmp, path) {
        // Never remove the good configuration if replacement fails. A sharing
        // violation must be reported, not turn a valid file into a missing one.
        let _ = fs::remove_file(&tmp);
        return Err(err).context("cannot atomically replace configuration");
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
        assert_eq!(
            normalize_exe("NVIDIA Overlay.exe").unwrap(),
            "nvidia overlay.exe"
        );
        assert!(normalize_exe("C:\\PicView.exe").is_err());
        assert!(normalize_exe("explorer.exe").is_err());
        assert!(normalize_exe("").is_err());
    }
    #[test]
    fn serde_defaults_and_rejects_unknown_fields() {
        assert!(serde_json::from_str::<Config>("{}").is_ok());
        assert!(serde_json::from_str::<Config>(r#"{"applications":[],"wrong":true}"#).is_err());
    }

    #[test]
    fn existing_rules_survive_preference_migration() {
        let cfg: Config =
            serde_json::from_str(r#"{"applications":["picview.exe"],"folders":["D:\\Private"]}"#)
                .unwrap();
        assert_eq!(cfg.applications, ["picview.exe"]);
        assert_eq!(cfg.folders, [PathBuf::from(r"D:\Private")]);
        assert!(!cfg.preferences.capture_native_resolution);
        let saved = serde_json::to_string(&cfg).unwrap();
        let restored: Config = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.folders, cfg.folders);
    }

    #[test]
    fn default_preferences_preserve_the_legacy_rules_schema() {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct LegacyConfig {
            applications: Vec<String>,
            folders: Vec<PathBuf>,
        }
        let mut cfg = Config {
            applications: vec!["picview.exe".into()],
            folders: vec![PathBuf::from(r"D:\Private")],
            ..Default::default()
        };
        let legacy: LegacyConfig =
            serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(legacy.applications, cfg.applications);
        assert_eq!(legacy.folders, cfg.folders);
        cfg.preferences.capture_cursor = true;
        let restored: Config = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert!(restored.preferences.capture_cursor);
    }

    #[test]
    fn atomic_save_keeps_good_configuration_when_windows_locks_it() {
        let root = std::env::temp_dir().join(format!("pawhide-config-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.json");
        let mut cfg = Config::default();
        cfg.applications.push("picview.exe".into());
        save_at(&path, &cfg).unwrap();
        cfg.applications.push("floorp.exe".into());
        save_at(&path, &cfg).unwrap();
        let original = fs::read(&path).unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let lock = fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&path)
                .unwrap();
            assert!(save_at(&path, &Config::default()).is_err());
            drop(lock);
            assert_eq!(fs::read(&path).unwrap(), original);
        }
        #[cfg(not(windows))]
        {
            assert!(!original.is_empty());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
