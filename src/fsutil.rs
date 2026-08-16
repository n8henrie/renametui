use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(crate) fn path_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn collision_key(path: &Path) -> PathBuf {
    let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
        return path.to_path_buf();
    };
    path.parent().map_or_else(
        || PathBuf::from(name.to_lowercase()),
        |parent| parent.join(name.to_lowercase()),
    )
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn collision_key(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(target_os = "macos")]
pub(crate) fn same_entry_alias(left: &Path, right: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    if collision_key(left) != collision_key(right) {
        return Ok(false);
    }

    let left_metadata = fs::symlink_metadata(left)?;
    let right_metadata = fs::symlink_metadata(right)?;
    let same_identity = left_metadata.dev() == right_metadata.dev()
        && left_metadata.ino() == right_metadata.ino();
    if !same_identity {
        return Ok(false);
    }
    if left_metadata.file_type().is_symlink() {
        return Ok(true);
    }

    Ok(fs::canonicalize(left)? == fs::canonicalize(right)?)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn same_entry_alias(left: &Path, right: &Path) -> io::Result<bool> {
    let _left_metadata = fs::symlink_metadata(left)?;
    let _right_metadata = fs::symlink_metadata(right)?;
    Ok(false)
}
