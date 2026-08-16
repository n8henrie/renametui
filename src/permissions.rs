use std::path::Path;

#[derive(Clone, Debug)]
pub(crate) struct PermissionChecker {
    state: PermissionState,
}

#[derive(Clone, Debug)]
enum PermissionState {
    #[cfg(unix)]
    Available(Identity),
    Unavailable(String),
    #[cfg(test)]
    Disabled,
}

impl PermissionChecker {
    pub(crate) fn detect() -> Self {
        Self {
            state: detect_permission_state(),
        }
    }

    #[cfg(test)]
    pub(crate) fn disabled() -> Self {
        Self {
            state: PermissionState::Disabled,
        }
    }

    pub(crate) fn warning_for(&self, source: &Path) -> Option<String> {
        match &self.state {
            #[cfg(unix)]
            PermissionState::Available(identity) => inspect_unix_permissions(identity, source),
            PermissionState::Unavailable(reason) => Some(format!(
                "permission preflight unavailable ({reason}); verify parent-directory access"
            )),
            #[cfg(test)]
            PermissionState::Disabled => None,
        }
    }
}

#[cfg(unix)]
fn detect_permission_state() -> PermissionState {
    match Identity::detect() {
        Ok(identity) => PermissionState::Available(identity),
        Err(error) => PermissionState::Unavailable(error.to_string()),
    }
}

#[cfg(not(unix))]
fn detect_permission_state() -> PermissionState {
    PermissionState::Unavailable(
        "mode-bit permission checks are only available on Unix".to_owned(),
    )
}

#[cfg(unix)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct Identity {
    uid: u32,
    groups: Vec<u32>,
}

#[cfg(unix)]
impl Identity {
    fn detect() -> std::io::Result<Self> {
        let uid = run_id(&["-u"])?
            .trim()
            .parse::<u32>()
            .map_err(invalid_identity_output)?;
        let groups = run_id(&["-G"])?
            .split_whitespace()
            .map(|group| group.parse::<u32>().map_err(invalid_identity_output))
            .collect::<std::io::Result<Vec<_>>>()?;

        Ok(Self { uid, groups })
    }
}

#[cfg(unix)]
fn run_id(arguments: &[&str]) -> std::io::Result<String> {
    use std::{io, process::Command};

    let output = Command::new("id").args(arguments).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "'id {}' exited with {}",
            arguments.join(" "),
            output.status
        )));
    }

    String::from_utf8(output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(unix)]
fn invalid_identity_output(error: std::num::ParseIntError) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("could not parse numeric output from id: {error}"),
    )
}

#[cfg(unix)]
fn inspect_unix_permissions(identity: &Identity, source: &Path) -> Option<String> {
    use std::{fs, os::unix::fs::MetadataExt};

    if identity.uid == 0 {
        return None;
    }

    let parent = source.parent()?;
    let parent_metadata = match fs::metadata(parent) {
        Ok(metadata) => metadata,
        Err(error) => {
            return Some(format!(
                "cannot inspect parent directory '{}': {error}",
                parent.display()
            ));
        }
    };
    let source_metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) => {
            return Some(format!(
                "cannot inspect source permissions '{}': {error}",
                source.display()
            ));
        }
    };

    warning_from_mode_bits(
        identity,
        PermissionFacts {
            parent_uid: parent_metadata.uid(),
            parent_gid: parent_metadata.gid(),
            parent_mode: parent_metadata.mode(),
            source_uid: source_metadata.uid(),
        },
    )
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PermissionFacts {
    parent_uid: u32,
    parent_gid: u32,
    parent_mode: u32,
    source_uid: u32,
}

#[cfg(unix)]
fn warning_from_mode_bits(identity: &Identity, facts: PermissionFacts) -> Option<String> {
    if identity.uid == 0 {
        return None;
    }

    let access_bits = if identity.uid == facts.parent_uid {
        (facts.parent_mode >> 6) & 0o7
    } else if identity.groups.contains(&facts.parent_gid) {
        (facts.parent_mode >> 3) & 0o7
    } else {
        facts.parent_mode & 0o7
    };

    if access_bits & 0o3 != 0o3 {
        return Some(
            "mode bits do not grant both write and search access to the parent directory"
                .to_owned(),
        );
    }

    let sticky = facts.parent_mode & 0o1000 != 0;
    let allowed_by_sticky_directory = identity.uid == facts.parent_uid
        || identity.uid == facts.source_uid;
    if sticky && !allowed_by_sticky_directory {
        return Some(
            "the sticky parent directory may forbid renaming an entry owned by another user"
                .to_owned(),
        );
    }

    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::{warning_from_mode_bits, Identity, PermissionFacts};

    #[test]
    fn owner_write_and_search_bits_are_sufficient() {
        let identity = Identity {
            uid: 1000,
            groups: vec![100],
        };
        let facts = PermissionFacts {
            parent_uid: 1000,
            parent_gid: 100,
            parent_mode: 0o40700,
            source_uid: 2000,
        };

        assert_eq!(warning_from_mode_bits(&identity, facts), None);
    }

    #[test]
    fn missing_group_search_permission_is_warned() {
        let identity = Identity {
            uid: 1000,
            groups: vec![100],
        };
        let facts = PermissionFacts {
            parent_uid: 2000,
            parent_gid: 100,
            parent_mode: 0o40620,
            source_uid: 1000,
        };

        let warning = warning_from_mode_bits(&identity, facts);
        assert!(warning.is_some_and(|message| message.contains("write and search")));
    }

    #[test]
    fn sticky_directory_warns_for_foreign_entries() {
        let identity = Identity {
            uid: 1000,
            groups: vec![100],
        };
        let facts = PermissionFacts {
            parent_uid: 2000,
            parent_gid: 100,
            parent_mode: 0o41777,
            source_uid: 3000,
        };

        let warning = warning_from_mode_bits(&identity, facts);
        assert!(warning.is_some_and(|message| message.contains("sticky")));
    }

    #[test]
    fn source_owner_can_rename_in_a_sticky_directory() {
        let identity = Identity {
            uid: 1000,
            groups: vec![100],
        };
        let facts = PermissionFacts {
            parent_uid: 2000,
            parent_gid: 100,
            parent_mode: 0o41777,
            source_uid: 1000,
        };

        assert_eq!(warning_from_mode_bits(&identity, facts), None);
    }
}
