use crate::{
    fsutil::{path_exists, same_entry_alias},
    plan::RenameAction,
    rename_order::{calculate, OrderError},
};
use std::{
    error::Error,
    fmt, fs, io,
    path::Path,
};

#[derive(Debug)]
pub(crate) struct RenameError {
    context: String,
    source: Option<io::Error>,
    rollback_failures: Vec<String>,
}

impl RenameError {
    fn message(context: impl Into<String>) -> Self {
        Self {
            context: context.into(),
            source: None,
            rollback_failures: Vec::new(),
        }
    }

    fn with_io(
        context: impl Into<String>,
        source: io::Error,
        rollback_failures: Vec<String>,
    ) -> Self {
        Self {
            context: context.into(),
            source: Some(source),
            rollback_failures,
        }
    }

    fn with_rollback(mut self, rollback_failures: Vec<String>) -> Self {
        self.rollback_failures.extend(rollback_failures);
        self
    }
}

impl fmt::Display for RenameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.context)?;
        if let Some(source) = &self.source {
            write!(formatter, ": {source}")?;
        }
        if !self.rollback_failures.is_empty() {
            write!(
                formatter,
                "; rollback problems: {}",
                self.rollback_failures.join("; ")
            )?;
        }
        Ok(())
    }
}

impl Error for RenameError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|error| error as &(dyn Error + 'static))
    }
}

pub(crate) fn execute(actions: &[RenameAction]) -> Result<(), RenameError> {
    execute_inner(actions, |_, _| Ok(()))
}

fn execute_inner(
    actions: &[RenameAction],
    mut before_rename: impl FnMut(usize, &RenameAction) -> io::Result<()>,
) -> Result<(), RenameError> {
    let order = preflight(actions)?;
    let mut completed = Vec::with_capacity(actions.len());

    for index in order {
        let action = &actions[index];
        if let Err(source) = before_rename(index, action) {
            let error = RenameError::with_io(
                "a pre-rename safety check failed",
                source,
                Vec::new(),
            );
            return Err(error.with_rollback(rollback(actions, &completed)));
        }
        if let Err(error) = rename_action(action) {
            return Err(error.with_rollback(rollback(actions, &completed)));
        }
        completed.push(index);
    }

    Ok(())
}

fn rename_action(action: &RenameAction) -> Result<(), RenameError> {
    match path_exists(&action.source) {
        Ok(true) => {}
        Ok(false) => {
            return Err(RenameError::message(format!(
                "source no longer exists: '{}'",
                action.source.display()
            )));
        }
        Err(source) => {
            return Err(RenameError::with_io(
                format!("failed to inspect source '{}'", action.source.display()),
                source,
                Vec::new(),
            ));
        }
    }

    move_without_overwrite(&action.source, &action.destination).map_err(|source| {
        RenameError::with_io(
            format!(
                "failed to rename '{}' as '{}'",
                action.source.display(),
                action.destination.display()
            ),
            source,
            Vec::new(),
        )
    })
}

fn preflight(actions: &[RenameAction]) -> Result<Vec<usize>, RenameError> {
    for action in actions {
        if action.source == action.destination {
            return Err(RenameError::message(format!(
                "source and destination are identical: '{}'",
                action.source.display()
            )));
        }
    }

    let order = calculate(actions).map_err(|error| order_error(actions, error))?;

    for action in actions {
        match path_exists(&action.source) {
            Ok(true) => {}
            Ok(false) => {
                return Err(RenameError::message(format!(
                    "source no longer exists: '{}'",
                    action.source.display()
                )));
            }
            Err(error) => {
                return Err(RenameError::with_io(
                    format!("failed to inspect source '{}'", action.source.display()),
                    error,
                    Vec::new(),
                ));
            }
        }
    }

    for action in actions {
        let destination_exists = path_exists(&action.destination).map_err(|error| {
            RenameError::with_io(
                format!(
                    "failed to inspect destination '{}'",
                    action.destination.display()
                ),
                error,
                Vec::new(),
            )
        })?;
        if !destination_exists {
            continue;
        }
        let occupied_by_moving_source = destination_is_moving_source(&action.destination, actions)
            .map_err(|error| {
                RenameError::with_io(
                    format!(
                        "failed to compare existing destination '{}'",
                        action.destination.display()
                    ),
                    error,
                    Vec::new(),
                )
            })?;
        if !occupied_by_moving_source {
            return Err(RenameError::message(format!(
                "destination already exists: '{}'",
                action.destination.display()
            )));
        }
    }

    Ok(order)
}

fn order_error(actions: &[RenameAction], error: OrderError) -> RenameError {
    match error {
        OrderError::DuplicateSource { index } => RenameError::message(format!(
            "source appears more than once: '{}'",
            actions[index].source.display()
        )),
        OrderError::DuplicateDestination { index } => RenameError::message(format!(
            "destination appears more than once: '{}'",
            actions[index].destination.display()
        )),
        OrderError::Cycle { indices } => {
            let sources = indices
                .iter()
                .map(|index| actions[*index].source.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            RenameError::message(format!(
                "rename cycle has no conflict-free direct execution order: {sources}"
            ))
        }
    }
}

fn destination_is_moving_source(
    destination: &Path,
    actions: &[RenameAction],
) -> io::Result<bool> {
    for action in actions {
        if action.source == destination || same_entry_alias(&action.source, destination)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn rollback(actions: &[RenameAction], completed: &[usize]) -> Vec<String> {
    let mut failures = Vec::new();

    for index in completed.iter().rev() {
        let action = &actions[*index];
        if let Err(error) = move_without_overwrite(&action.destination, &action.source) {
            failures.push(format!(
                "could not restore '{}' from '{}': {error}",
                action.source.display(),
                action.destination.display()
            ));
        }
    }

    failures
}

fn move_without_overwrite(source: &Path, destination: &Path) -> io::Result<()> {
    if path_exists(destination)?
        && source != destination
        && !same_entry_alias(source, destination)?
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("destination '{}' already exists", destination.display()),
        ));
    }
    fs::rename(source, destination)
}

#[cfg(test)]
mod tests {
    use super::{execute, execute_inner};
    use crate::plan::RenameAction;
    use std::{error::Error, fs};

    #[test]
    fn simple_rename_moves_the_entry() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("transaction-simple")?;
        let source = directory.path().join("before");
        let destination = directory.path().join("after");
        fs::write(&source, b"contents")?;

        execute(&[RenameAction {
            source: source.clone(),
            destination: destination.clone(),
        }])?;

        assert!(!source.exists());
        assert_eq!(fs::read(destination)?, b"contents");
        Ok(())
    }

    #[test]
    fn rename_cycles_are_refused_without_modifying_entries() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("transaction-swap")?;
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        fs::write(&first, b"one")?;
        fs::write(&second, b"two")?;

        let result = execute(&[
            RenameAction {
                source: first.clone(),
                destination: second.clone(),
            },
            RenameAction {
                source: second.clone(),
                destination: first.clone(),
            },
        ]);

        assert!(result.is_err());
        assert_eq!(fs::read(first)?, b"one");
        assert_eq!(fs::read(second)?, b"two");
        Ok(())
    }

    #[test]
    fn rename_chains_are_executed_in_dependency_order() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("transaction-chain")?;
        let first = directory.path().join("a");
        let second = directory.path().join("aa");
        let third = directory.path().join("aaa");
        fs::write(&first, b"first")?;
        fs::write(&second, b"second")?;

        execute(&[
            RenameAction {
                source: first.clone(),
                destination: second.clone(),
            },
            RenameAction {
                source: second.clone(),
                destination: third.clone(),
            },
        ])?;

        assert!(!first.exists());
        assert_eq!(fs::read(second)?, b"first");
        assert_eq!(fs::read(third)?, b"second");
        Ok(())
    }

    #[test]
    fn existing_unmoved_destinations_are_refused() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("transaction-existing")?;
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        fs::write(&source, b"source")?;
        fs::write(&destination, b"destination")?;

        let result = execute(&[RenameAction {
            source: source.clone(),
            destination: destination.clone(),
        }]);

        assert!(result.is_err());
        assert_eq!(fs::read(source)?, b"source");
        assert_eq!(fs::read(destination)?, b"destination");
        Ok(())
    }

    #[test]
    fn a_destination_created_after_preflight_is_not_overwritten() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("transaction-race")?;
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        fs::write(&source, b"source")?;
        let action = RenameAction {
            source: source.clone(),
            destination: destination.clone(),
        };

        let result = execute_inner(&[action], |_, _| fs::write(&destination, b"racer"));

        assert!(result.is_err());
        assert_eq!(fs::read(source)?, b"source");
        assert_eq!(fs::read(destination)?, b"racer");
        Ok(())
    }

    #[test]
    fn late_conflict_rolls_back_completed_direct_renames() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("transaction-partial-rollback")?;
        let first_source = directory.path().join("first-source");
        let first_destination = directory.path().join("first-destination");
        let second_source = directory.path().join("second-source");
        let second_destination = directory.path().join("second-destination");
        fs::write(&first_source, b"first")?;
        fs::write(&second_source, b"second")?;
        let actions = [
            RenameAction {
                source: first_source.clone(),
                destination: first_destination.clone(),
            },
            RenameAction {
                source: second_source.clone(),
                destination: second_destination.clone(),
            },
        ];

        let result = execute_inner(&actions, |index, _| {
            if index == 1 {
                fs::write(&second_destination, b"racer")?;
            }
            Ok(())
        });

        assert!(result.is_err());
        assert_eq!(fs::read(first_source)?, b"first");
        assert_eq!(fs::read(second_source)?, b"second");
        assert!(!first_destination.exists());
        assert_eq!(fs::read(second_destination)?, b"racer");
        Ok(())
    }
}
