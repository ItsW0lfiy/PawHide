//! Directory rules store normal, readable absolute paths (not Win32 extended paths).
//! Explorer can provide its active folder through ShellWindows; document apps
//! must send an exact file path (titles are not a privacy-safe source).
use anyhow::{Context, Result, ensure};
use std::{fs, path::{Path, PathBuf}};

/// Remove the Windows canonicalization namespace from user-visible/config paths.
/// Leave UNC paths intact, converting `\\?\UNC\server` back to `\\server`.
pub fn display_path(path: &Path) -> PathBuf {
    #[cfg(windows)] {
        let s = path.to_string_lossy();
        if let Some(unc) = s.strip_prefix(r"\\?\UNC\") { return PathBuf::from(format!(r"\\{unc}")); }
        if let Some(drive) = s.strip_prefix(r"\\?\") { return PathBuf::from(drive); }
    }
    path.to_path_buf()
}

/// Existing folders are validated before registration. Paths that do not exist
/// produce an actionable error instead of silently claiming they're protected.
pub fn validated_folder(path: &Path) -> Result<PathBuf> {
    ensure!(!path.as_os_str().is_empty(), "enter a folder path");
    let absolute = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir()?.join(path) };
    ensure!(absolute.is_dir(), "folder does not exist or is inaccessible: {}", absolute.display());
    Ok(display_path(&fs::canonicalize(&absolute)
        .with_context(|| format!("cannot access folder {}", absolute.display()))?))
}

/// Component-aware containment; on Windows, comparisons ignore case. The root
/// itself counts as a protected directory for Explorer windows.
fn contains_path(root: &Path, candidate: &Path) -> bool {
    #[cfg(windows)] {
        let a = display_path(root).to_string_lossy().replace('/', "\\");
        let b = display_path(candidate).to_string_lossy().replace('/', "\\");
        let a = a.trim_end_matches('\\').to_lowercase();
        let b = b.trim_end_matches('\\').to_lowercase();
        return b == a || b.strip_prefix(&a).is_some_and(|rest| rest.starts_with('\\'));
    }
    #[cfg(not(windows))] { candidate == root || candidate.starts_with(root) }
}

pub fn match_directory<'a>(folders: &'a [PathBuf], directory: &Path) -> Result<Option<&'a PathBuf>> {
    // Resolve symlinks on both sides, including rules saved by older versions.
    let directory = fs::canonicalize(directory).with_context(|| format!("cannot access directory {}", directory.display()))?;
    ensure!(directory.is_dir(), "not a folder: {}", directory.display());
    Ok(folders.iter().find(|root| {
        fs::canonicalize(root).is_ok_and(|resolved| contains_path(&resolved, &directory))
    }))
}

pub fn match_document<'a>(folders: &'a [PathBuf], file: &Path) -> Result<Option<&'a PathBuf>> {
    let file = fs::canonicalize(file).with_context(|| format!("cannot resolve document {}", file.display()))?;
    ensure!(file.is_file(), "not a regular file: {}", file.display());
    Ok(folders.iter().find(|root| {
        fs::canonicalize(root).is_ok_and(|resolved| contains_path(&resolved, &file))
    }))
}

/// Old configurations can contain Windows `\\?\` paths. Convert before display
/// and de-duplication without throwing away inaccessible removable-drive rules.
pub fn clean_saved_paths(folders: &mut Vec<PathBuf>) {
    for path in folders.iter_mut() { *path = display_path(path); }
    folders.sort();
    folders.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_and_document_rules() {
        let base = std::env::temp_dir().join(format!("pawhide-path-test-{}", std::process::id()));
        let root = base.join("Private");
        let sub = root.join("Sub");
        let neighbor = base.join("Private2");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(&neighbor).unwrap();
        let inside = sub.join("secret.md");
        let outside = neighbor.join("other.md");
        fs::write(&inside, "inside").unwrap();
        fs::write(&outside, "outside").unwrap();
        let rules = vec![validated_folder(&root).unwrap()];
        assert!(match_directory(&rules, &root).unwrap().is_some());
        assert!(match_directory(&rules, &sub).unwrap().is_some());
        assert!(match_directory(&rules, &neighbor).unwrap().is_none());
        assert!(match_document(&rules, &inside).unwrap().is_some());
        assert!(match_document(&rules, &outside).unwrap().is_none());
        assert!(match_document(&rules, &base.join("missing.md")).is_err());
        fs::remove_dir_all(&base).unwrap();
    }
    #[test]
    fn invalid_registration_is_explicit() {
        assert!(validated_folder(Path::new("/a/folder/that/does/not/exist/pawhide")).is_err());
    }
}
