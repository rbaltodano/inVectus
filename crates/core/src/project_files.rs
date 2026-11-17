//! Source/cache file policy captured with its key, without holding a database connection.
//! New projects encrypt; older ones keep their original format until explicit migration.

use crate::{Project, keys::DataKey, project::ProjectConfig};
use anyhow::{Context, Result, bail};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct ProjectFiles {
    root: PathBuf,
    supplied_root: PathBuf,
    identity: Option<Arc<(String, DataKey)>>,
}

impl std::fmt::Debug for ProjectFiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectFiles")
            .field("root", &self.root)
            .field("encrypted", &self.encrypted())
            .finish_non_exhaustive()
    }
}

impl ProjectFiles {
    pub fn for_project(project: &Project) -> Result<Self> {
        Self::from_config(&project.root, &project.config)
    }

    pub fn at(root: &Path) -> Result<Self> {
        let (root, config) = Project::config_at(root)?;
        Self::from_config(&root, &config)
    }

    fn from_config(root: &Path, config: &ProjectConfig) -> Result<Self> {
        crate::project::validate_file_encryption(root, config)?;
        let supplied_root = if root.is_absolute() {
            root.to_owned()
        } else {
            std::env::current_dir()?.join(root)
        };
        let root = root
            .canonicalize()
            .context("resolving project files folder")?;
        let identity = if config.files_encrypted {
            let id = config
                .project_id
                .as_ref()
                .context("encrypted files need a project id")?;
            Some(Arc::new((id.clone(), config.data_key()?)))
        } else {
            None
        };
        Ok(Self {
            root,
            supplied_root,
            identity,
        })
    }

    pub fn encrypted(&self) -> bool {
        self.identity.is_some()
    }

    /// Only project-owned sources and cache files; no exports, database or arbitrary paths.
    /// Reject links rather than allowing an encrypted write or read to escape the project.
    fn location(&self, path: &Path) -> Result<(PathBuf, String)> {
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir()?.join(path)
        };
        let relative = absolute
            .strip_prefix(&self.supplied_root)
            .or_else(|_| absolute.strip_prefix(&self.root))
            .context("file is outside this project")?;
        let parts: Vec<_> = relative.components().collect();
        if parts.len() < 2
            || !matches!(parts[0], Component::Normal(p) if p == "sources" || p == "cache")
            || parts.iter().any(|p| !matches!(p, Component::Normal(_)))
        {
            bail!("only files inside this project's sources or cache folders are allowed");
        }
        let mut target = self.root.clone();
        let mut names = Vec::new();
        for part in parts {
            let Component::Normal(name) = part else {
                unreachable!()
            };
            names.push(name.to_str().context("project file name is not UTF-8")?);
            target.push(name);
            match std::fs::symlink_metadata(&target) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    bail!("project source/cache paths must not contain symlinks")
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok((target, names.join("/")))
    }

    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        use std::io::Read;
        let (path, relative) = self.location(path)?;
        let mut file = std::fs::File::open(&path)?;
        // Bound allocation even for a malicious length or concurrently growing file.
        const MAX_STORED: u64 = 513 * 1024 * 1024;
        let mut bytes = Vec::new();
        (&mut file).take(MAX_STORED + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_STORED {
            bail!("project source/cache file is too large");
        }
        match &self.identity {
            Some(identity) => {
                crate::encrypted_file::open(&identity.1, &identity.0, &relative, &bytes)
            }
            None if crate::encrypted_file::is_encrypted(&bytes) => {
                bail!("encrypted project file requires its encryption policy and key")
            }
            None => Ok(bytes),
        }
    }

    pub fn write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let (path, relative) = self.location(path)?;
        let stored = match &self.identity {
            Some(identity) => {
                crate::encrypted_file::seal(&identity.1, &identity.0, &relative, bytes)?
            }
            None => bytes.to_vec(),
        };
        crate::private::write(&path, &stored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_policy_refuses_plaintext_downgrades_and_path_substitution() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::create(&dir.path().join("files")).unwrap();
        let files = ProjectFiles::for_project(&project).unwrap();
        assert!(files.encrypted());
        let path = project.root.join("sources/notion/data.csv");
        files.write(&path, b"PRIVATE_SOURCE_MARKER").unwrap();
        assert_eq!(files.read(&path).unwrap(), b"PRIVATE_SOURCE_MARKER");
        let ciphertext = std::fs::read(&path).unwrap();
        assert!(
            !ciphertext
                .windows(b"PRIVATE_SOURCE_MARKER".len())
                .any(|w| w == b"PRIVATE_SOURCE_MARKER")
        );
        assert!(
            files
                .write(&project.root.join("exports/leak.csv"), b"private")
                .is_err()
        );
        assert!(
            files
                .write(&project.root.join("sources/../leak.csv"), b"private")
                .is_err()
        );
        assert!(
            files
                .write(&dir.path().join("outside.csv"), b"private")
                .is_err()
        );
        let other = path.with_file_name("other.csv");
        std::fs::write(&other, &ciphertext).unwrap();
        assert!(files.read(&other).is_err());
        std::fs::write(&path, b"plaintext replacement").unwrap();
        assert!(files.read(&path).is_err());
        let mut config = project.config.clone();
        config.files_encrypted = false;
        crate::project::write_json_atomic(&project.root.join("project.json"), &config).unwrap();
        assert!(ProjectFiles::at(&project.root).is_err());
        assert!(Project::config_at(&project.root).is_err());
    }

    #[test]
    fn legacy_phase_three_files_remain_plaintext_until_migrated() {
        let dir = tempfile::tempdir().unwrap();
        // A phase-3 project has an encrypted database, but no files_encrypted field. No key
        // access or database open is needed to read its existing source/cache files.
        std::fs::write(dir.path().join("project.json"), br#"{"format_version":1,"name":"Legacy phase 3","network_enabled":false,"store_encrypted":true}"#).unwrap();
        let files = ProjectFiles::at(dir.path()).unwrap();
        assert!(!files.encrypted());
        let path = dir.path().join("sources/notion/data.csv");
        files.write(&path, b"title\nlegacy\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"title\nlegacy\n");
        assert_eq!(files.read(&path).unwrap(), b"title\nlegacy\n");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_files_and_parents_are_refused() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let project = Project::create(&dir.path().join("links")).unwrap();
        let files = ProjectFiles::for_project(&project).unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::create_dir(project.root.join("sources")).unwrap();
        symlink(&outside, project.root.join("sources/link")).unwrap();
        assert!(
            files
                .write(&project.root.join("sources/link/a.csv"), b"secret")
                .is_err()
        );
        let outside_file = outside.join("b.csv");
        std::fs::write(&outside_file, b"unchanged").unwrap();
        symlink(&outside_file, project.root.join("sources/b.csv")).unwrap();
        assert!(files.read(&project.root.join("sources/b.csv")).is_err());
        assert!(
            files
                .write(&project.root.join("sources/b.csv"), b"secret")
                .is_err()
        );
        assert_eq!(std::fs::read(&outside_file).unwrap(), b"unchanged");
    }
}
