//! Explicit document-path rules. Window titles alone cannot establish which
//! file is open, so an application must report the actual path and HWND.
use anyhow::{Context, Result, ensure};
use std::{fs, path::{Path, PathBuf}};

pub fn validated_folder(path: &Path) -> Result<PathBuf> {
    let resolved = fs::canonicalize(path)
        .with_context(|| format!("cannot resolve directory {}", path.display()))?;
    ensure!(resolved.is_dir(), "not a directory: {}", path.display());
    Ok(resolved)
}

/// Resolve the actual on-disk path before testing folder containment, avoiding
/// `C:\\Private` matching `C:\\Private2` and symlink escapes.
pub fn match_document<'a>(folders: &'a [PathBuf], file: &Path) -> Result<Option<&'a PathBuf>> {
    let file = fs::canonicalize(file).with_context(|| format!("cannot resolve document {}", file.display()))?;
    ensure!(file.is_file(), "not a regular file: {}", file.display());
    Ok(folders.iter().find(|root| {
        #[cfg(windows)] {
            let root = root.to_string_lossy().to_lowercase();
            let document = file.to_string_lossy().to_lowercase();
            document.starts_with(&format!("{}\\", root.trim_end_matches('\\')))
        }
        #[cfg(not(windows))] { file.starts_with(root) && file != **root }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_uses_component_boundaries_and_resolves_symlinks() {
        let base = std::env::temp_dir().join(format!("pawhide-path-test-{}", std::process::id()));
        let root = base.join("Private");
        let neighboring = base.join("Private2");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&neighboring).unwrap();
        let within = root.join("secret.md");
        let outside = neighboring.join("other.md");
        fs::write(&within, "inside").unwrap();
        fs::write(&outside, "outside").unwrap();
        let rules = vec![validated_folder(&root).unwrap()];
        assert!(match_document(&rules, &within).unwrap().is_some());
        assert!(match_document(&rules, &outside).unwrap().is_none());
        assert!(match_document(&rules, &base.join("nonexistent.md")).is_err());
        fs::remove_dir_all(&base).unwrap();
    }
}
