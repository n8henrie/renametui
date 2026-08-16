use std::{
    collections::HashSet,
    fs, io,
    path::{absolute, Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Entry {
    pub(crate) path: PathBuf,
    pub(crate) kind: EntryKind,
}

impl Entry {
    pub(crate) fn file_name(&self) -> Option<&std::ffi::OsStr> {
        self.path.file_name()
    }
}

pub(crate) fn load(paths: Vec<PathBuf>) -> io::Result<Vec<Entry>> {
    let resolved = if paths.is_empty() {
        current_directory_entries()?
    } else {
        explicit_paths(paths)?
    };

    let mut seen = HashSet::new();
    let mut entries = Vec::with_capacity(resolved.len());

    for path in resolved {
        if !seen.insert(path.clone()) {
            continue;
        }

        if path.file_name().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("cannot rename a filesystem root: {}", path.display()),
            ));
        }

        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| contextual_error(&error, &path, "inspect"))?;
        let kind = if metadata.file_type().is_dir() {
            EntryKind::Directory
        } else {
            EntryKind::File
        };
        entries.push(Entry { path, kind });
    }

    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn current_directory_entries() -> io::Result<Vec<PathBuf>> {
    let current_directory = std::env::current_dir()?;
    let directory = fs::read_dir(&current_directory).map_err(|error| {
        contextual_error(&error, &current_directory, "read the current directory")
    })?;

    directory
        .map(|entry| {
            entry
                .map(|value| value.path())
                .map_err(|error| contextual_error(&error, &current_directory, "read an entry in"))
        })
        .collect()
}

fn explicit_paths(paths: Vec<PathBuf>) -> io::Result<Vec<PathBuf>> {
    paths
        .into_iter()
        .map(|path| {
            absolute(&path).map_err(|error| contextual_error(&error, &path, "resolve"))
        })
        .collect()
}

fn contextual_error(error: &io::Error, path: &Path, operation: &str) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("failed to {operation} '{}': {error}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::{load, EntryKind};
    use std::{error::Error, fs};

    #[test]
    fn explicit_regular_files_are_loaded() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("entry-load")?;
        let path = directory.path().join("example.txt");
        fs::write(&path, b"example")?;

        let entries = load(vec![path.clone()])?;

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, path);
        assert_eq!(entries[0].kind, EntryKind::File);
        Ok(())
    }

    #[test]
    fn duplicate_paths_are_loaded_once() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("entry-deduplicate")?;
        let path = directory.path().join("example.txt");
        fs::write(&path, b"example")?;

        let entries = load(vec![path.clone(), path])?;

        assert_eq!(entries.len(), 1);
        Ok(())
    }
}
