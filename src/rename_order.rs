use crate::{fsutil::collision_key, plan::RenameAction};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OrderError {
    DuplicateSource { index: usize },
    DuplicateDestination { index: usize },
    Cycle { indices: Vec<usize> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyKind {
    Source,
    Destination,
}

pub(crate) fn calculate(actions: &[RenameAction]) -> Result<Vec<usize>, OrderError> {
    let sources = unique_indices(actions, KeyKind::Source)?;
    unique_indices(actions, KeyKind::Destination)?;

    let mut has_unresolved_dependency = vec![false; actions.len()];
    let mut dependents = vec![Vec::new(); actions.len()];

    for (index, action) in actions.iter().enumerate() {
        let dependency = sources
            .get(&collision_key(&action.destination))
            .copied()
            .filter(|candidate| *candidate != index);
        if let Some(dependency) = dependency {
            has_unresolved_dependency[index] = true;
            dependents[dependency].push(index);
        }
    }

    let mut ready = has_unresolved_dependency
        .iter()
        .enumerate()
        .filter_map(|(index, blocked)| (!*blocked).then_some(index))
        .collect::<VecDeque<_>>();
    let mut order = Vec::with_capacity(actions.len());

    while let Some(index) = ready.pop_front() {
        order.push(index);
        for &dependent in &dependents[index] {
            has_unresolved_dependency[dependent] = false;
            ready.push_back(dependent);
        }
    }

    if order.len() == actions.len() {
        return Ok(order);
    }

    let indices = has_unresolved_dependency
        .iter()
        .enumerate()
        .filter_map(|(index, blocked)| (*blocked).then_some(index))
        .collect();
    Err(OrderError::Cycle { indices })
}

fn unique_indices(
    actions: &[RenameAction],
    kind: KeyKind,
) -> Result<HashMap<PathBuf, usize>, OrderError> {
    let mut indices = HashMap::with_capacity(actions.len());

    for (index, action) in actions.iter().enumerate() {
        let key = match kind {
            KeyKind::Source => collision_key(&action.source),
            KeyKind::Destination => collision_key(&action.destination),
        };
        if indices.insert(key, index).is_some() {
            return match kind {
                KeyKind::Source => Err(OrderError::DuplicateSource { index }),
                KeyKind::Destination => Err(OrderError::DuplicateDestination { index }),
            };
        }
    }

    Ok(indices)
}

#[cfg(test)]
mod tests {
    use super::{calculate, OrderError};
    use crate::plan::RenameAction;
    use std::path::PathBuf;

    fn action(source: &str, destination: &str) -> RenameAction {
        RenameAction {
            source: PathBuf::from(source),
            destination: PathBuf::from(destination),
        }
    }

    #[test]
    fn dependencies_are_placed_before_their_consumers() {
        let actions = [action("a", "aa"), action("aa", "aaa")];

        let order = calculate(&actions);

        assert_eq!(order, Ok(vec![1, 0]));
    }

    #[test]
    fn cycles_are_reported_in_memory() {
        let actions = [action("first", "second"), action("second", "first")];

        let order = calculate(&actions);

        assert_eq!(order, Err(OrderError::Cycle { indices: vec![0, 1] }));
    }

    #[test]
    fn every_disjoint_cycle_is_reported() {
        let actions = [
            action("a", "b"),
            action("b", "a"),
            action("c", "d"),
            action("d", "c"),
        ];

        let order = calculate(&actions);

        assert_eq!(
            order,
            Err(OrderError::Cycle {
                indices: vec![0, 1, 2, 3],
            })
        );
    }
}
