use crate::{
    entry::{Entry, EntryKind},
    fsutil::{collision_key, path_exists, same_entry_alias},
    permissions::PermissionChecker,
    rename_order::{calculate, OrderError},
};
use regex::Regex;
use std::{
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SelectionFilter {
    Files,
    Directories,
    Both,
}

impl SelectionFilter {
    pub(crate) fn includes(self, kind: EntryKind) -> bool {
        match self {
            Self::Files => kind == EntryKind::File,
            Self::Directories => kind == EntryKind::Directory,
            Self::Both => true,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Directories => "folders",
            Self::Both => "both",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IssueLevel {
    Warning,
    Conflict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Issue {
    pub(crate) level: IssueLevel,
    pub(crate) message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RowState {
    NotSelected,
    Waiting,
    Unchanged,
    Ready,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreviewRow {
    pub(crate) kind: EntryKind,
    pub(crate) before: String,
    pub(crate) after: String,
    pub(crate) match_ranges: Vec<Range<usize>>,
    pub(crate) state: RowState,
    pub(crate) issues: Vec<Issue>,
}

impl PreviewRow {
    pub(crate) fn has_conflict(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.level == IssueLevel::Conflict)
    }

    pub(crate) fn has_warning(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.level == IssueLevel::Warning)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RenameAction {
    pub(crate) source: PathBuf,
    pub(crate) destination: PathBuf,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Plan {
    pub(crate) rows: Vec<PreviewRow>,
    pub(crate) actions: Vec<RenameAction>,
    pub(crate) regex_error: Option<String>,
    pub(crate) pattern_is_empty: bool,
}

impl Plan {
    pub(crate) fn conflict_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.has_conflict())
            .count()
    }

    pub(crate) fn warning_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.has_warning())
            .count()
    }

    pub(crate) fn unchanged_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.state == RowState::Unchanged)
            .count()
    }

    pub(crate) fn ready_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.state == RowState::Ready && !row.has_conflict())
            .count()
    }

    pub(crate) fn can_execute(&self) -> bool {
        !self.pattern_is_empty
            && self.regex_error.is_none()
            && self.conflict_count() == 0
            && !self.actions.is_empty()
    }

    pub(crate) fn blocking_details(&self) -> Vec<String> {
        if self.pattern_is_empty {
            return vec!["Enter a regular expression before reviewing the plan.".to_owned()];
        }
        if let Some(error) = &self.regex_error {
            return vec![format!("Invalid regex: {error}")];
        }

        let conflicts = self.issue_details(IssueLevel::Conflict, "CONFLICT");
        if !conflicts.is_empty() {
            return conflicts;
        }
        if self.actions.is_empty() {
            return vec![
                "The regular expression does not change any selected names.".to_owned(),
            ];
        }
        Vec::new()
    }

    pub(crate) fn warning_details(&self) -> Vec<String> {
        self.issue_details(IssueLevel::Warning, "WARN")
    }

    fn issue_details(&self, level: IssueLevel, label: &str) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|row| {
                let messages: Vec<&str> = row
                    .issues
                    .iter()
                    .filter(|issue| issue.level == level)
                    .map(|issue| issue.message.as_str())
                    .collect();
                if messages.is_empty() {
                    return None;
                }

                Some(format!(
                    "{} -> {}: {label}: {}",
                    row.before,
                    row.after,
                    messages.join("; ")
                ))
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    row_index: usize,
    kind: EntryKind,
    action: RenameAction,
}

pub(crate) fn build(
    entries: &[Entry],
    filter: SelectionFilter,
    pattern: &str,
    replacement: &str,
    permissions: &PermissionChecker,
) -> Plan {
    let pattern_is_empty = pattern.is_empty();
    let (compiled, regex_error) = if pattern_is_empty {
        (None, None)
    } else {
        match Regex::new(pattern) {
            Ok(regex) => (Some(regex), None),
            Err(error) => (None, Some(error.to_string())),
        }
    };

    let (mut rows, candidates) = create_previews(
        entries,
        filter,
        compiled.as_ref(),
        pattern_is_empty,
        replacement,
        permissions,
    );
    mark_duplicate_sources(&mut rows, &candidates);
    mark_duplicate_destinations(&mut rows, &candidates);
    mark_existing_destinations(&mut rows, &candidates);
    mark_nested_sources(&mut rows, &candidates);
    let actions: Vec<RenameAction> = candidates
        .iter()
        .map(|candidate| candidate.action.clone())
        .collect();
    mark_rename_cycles(&mut rows, &candidates, &actions);

    Plan {
        rows,
        actions,
        regex_error,
        pattern_is_empty,
    }
}

fn create_previews(
    entries: &[Entry],
    filter: SelectionFilter,
    regex: Option<&Regex>,
    pattern_is_empty: bool,
    replacement: &str,
    permissions: &PermissionChecker,
) -> (Vec<PreviewRow>, Vec<Candidate>) {
    let mut rows = Vec::with_capacity(entries.len());
    let mut candidates = Vec::new();

    for entry in entries {
        let row_index = rows.len();
        let (row, action) = preview_entry(
            entry,
            filter,
            regex,
            pattern_is_empty,
            replacement,
            permissions,
        );
        rows.push(row);
        if let Some(action) = action {
            candidates.push(Candidate {
                row_index,
                kind: entry.kind,
                action,
            });
        }
    }

    (rows, candidates)
}

fn preview_entry(
    entry: &Entry,
    filter: SelectionFilter,
    regex: Option<&Regex>,
    pattern_is_empty: bool,
    replacement: &str,
    permissions: &PermissionChecker,
) -> (PreviewRow, Option<RenameAction>) {
    let before = entry
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let mut row = PreviewRow {
        kind: entry.kind,
        before: before.clone(),
        after: before,
        match_ranges: Vec::new(),
        state: RowState::Waiting,
        issues: Vec::new(),
    };

    if !filter.includes(entry.kind) {
        row.state = RowState::NotSelected;
        return (row, None);
    }
    if pattern_is_empty {
        return (row, None);
    }
    let Some(regex) = regex else {
        return (row, None);
    };

    let Some(name) = entry.file_name().and_then(std::ffi::OsStr::to_str) else {
        add_issue(
            &mut row,
            IssueLevel::Conflict,
            "the filename is not valid UTF-8 and cannot be matched by regex".to_owned(),
        );
        return (row, None);
    };
    row.match_ranges = regex
        .find_iter(name)
        .map(|regex_match| regex_match.start()..regex_match.end())
        .filter(|range| !range.is_empty())
        .collect();
    let after = regex.replace_all(name, replacement).into_owned();
    row.after.clone_from(&after);

    if after == name {
        row.state = RowState::Unchanged;
        return (row, None);
    }
    if let Some(reason) = invalid_basename_reason(&after) {
        add_issue(&mut row, IssueLevel::Conflict, reason);
        return (row, None);
    }
    match path_exists(&entry.path) {
        Ok(true) => {}
        Ok(false) => {
            add_issue(
                &mut row,
                IssueLevel::Conflict,
                "the source no longer exists".to_owned(),
            );
            return (row, None);
        }
        Err(error) => {
            add_issue(
                &mut row,
                IssueLevel::Conflict,
                format!("could not inspect the source: {error}"),
            );
            return (row, None);
        }
    }
    let Some(parent) = entry.path.parent() else {
        add_issue(
            &mut row,
            IssueLevel::Conflict,
            "the source path has no parent directory".to_owned(),
        );
        return (row, None);
    };

    row.state = RowState::Ready;
    if let Some(warning) = permissions.warning_for(&entry.path) {
        add_issue(&mut row, IssueLevel::Warning, warning);
    }
    let action = RenameAction {
        source: entry.path.clone(),
        destination: parent.join(after),
    };
    (row, Some(action))
}

fn invalid_basename_reason(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("the replacement produces an empty filename".to_owned());
    }
    if matches!(name, "." | "..") {
        return Some(format!("'{name}' is not a renameable filename"));
    }
    if name.contains('/') {
        return Some("the replacement contains a path separator".to_owned());
    }
    if name.contains('\0') {
        return Some("the replacement contains a NUL byte".to_owned());
    }
    None
}

fn mark_duplicate_sources(rows: &mut [PreviewRow], candidates: &[Candidate]) {
    let mut sources: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    for candidate in candidates {
        sources
            .entry(collision_key(&candidate.action.source))
            .or_default()
            .push(candidate.row_index);
    }

    for indices in sources.values().filter(|indices| indices.len() > 1) {
        for &row_index in indices {
            add_row_issue(
                rows,
                row_index,
                IssueLevel::Conflict,
                "the same source was selected more than once".to_owned(),
            );
        }
    };
}

fn mark_duplicate_destinations(rows: &mut [PreviewRow], candidates: &[Candidate]) {
    let mut destinations: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    for candidate in candidates {
        destinations
            .entry(collision_key(&candidate.action.destination))
            .or_default()
            .push(candidate.row_index);
    }

    for indices in destinations.values().filter(|indices| indices.len() > 1) {
        for &row_index in indices {
            add_row_issue(
                rows,
                row_index,
                IssueLevel::Conflict,
                "multiple entries would have the same destination".to_owned(),
            );
        }
    };
}

fn mark_existing_destinations(rows: &mut [PreviewRow], candidates: &[Candidate]) {
    for candidate in candidates {
        let destination = &candidate.action.destination;
        let conflict = match path_exists(destination) {
            Ok(false) => None,
            Ok(true) => match destination_is_moving_source(destination, candidates) {
                Ok(true) => None,
                Ok(false) => Some(format!(
                    "destination '{}' already exists and is not moving",
                    destination.display()
                )),
                Err(error) => Some(format!(
                    "could not compare an existing destination '{}': {error}",
                    destination.display()
                )),
            },
            Err(error) => Some(format!(
                "could not inspect destination '{}': {error}",
                destination.display()
            )),
        };

        if let Some(message) = conflict {
            add_row_issue(
                rows,
                candidate.row_index,
                IssueLevel::Conflict,
                message,
            );
        }
    };
}

fn destination_is_moving_source(
    destination: &Path,
    candidates: &[Candidate],
) -> std::io::Result<bool> {
    for candidate in candidates {
        let source = &candidate.action.source;
        if source == destination || same_entry_alias(source, destination)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn mark_nested_sources(rows: &mut [PreviewRow], candidates: &[Candidate]) {
    for directory in candidates
        .iter()
        .filter(|candidate| candidate.kind == EntryKind::Directory)
    {
        for descendant in candidates.iter().filter(|candidate| {
            candidate.action.source != directory.action.source
                && candidate.action.source.starts_with(&directory.action.source)
        }) {
            let message = format!(
                "nested selections are unsafe: '{}' contains another selected source",
                directory.action.source.display()
            );
            add_row_issue(
                rows,
                directory.row_index,
                IssueLevel::Conflict,
                message.clone(),
            );
            add_row_issue(
                rows,
                descendant.row_index,
                IssueLevel::Conflict,
                message,
            );
        }
    };
}

fn mark_rename_cycles(
    rows: &mut [PreviewRow],
    candidates: &[Candidate],
    actions: &[RenameAction],
) {
    let Err(OrderError::Cycle { indices }) = calculate(actions) else {
        return;
    };

    for index in indices {
        if let Some(candidate) = candidates.get(index) {
            add_row_issue(
                rows,
                candidate.row_index,
                IssueLevel::Conflict,
                "rename cycle has no conflict-free direct execution order".to_owned(),
            );
        }
    }
}

fn add_row_issue(
    rows: &mut [PreviewRow],
    row_index: usize,
    level: IssueLevel,
    message: String,
) {
    if let Some(row) = rows.get_mut(row_index) {
        add_issue(row, level, message);
    };
}

fn add_issue(row: &mut PreviewRow, level: IssueLevel, message: String) {
    let issue = Issue { level, message };
    if !row.issues.contains(&issue) {
        row.issues.push(issue);
    };
}

#[cfg(test)]
mod tests {
    use super::{build, IssueLevel, SelectionFilter};
    use crate::{
        entry::{Entry, EntryKind},
        permissions::PermissionChecker,
    };
    use std::{error::Error, fs, path::PathBuf};

    fn entry(path: PathBuf, kind: EntryKind) -> Entry {
        Entry { path, kind }
    }

    #[test]
    fn capture_groups_are_shown_in_the_preview() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("plan-captures")?;
        let source = directory.path().join("report-2026.txt");
        fs::write(&source, b"report")?;
        let entries = vec![entry(source, EntryKind::File)];

        let plan = build(
            &entries,
            SelectionFilter::Both,
            r"^(.+)-(\d{4})\.txt$",
            "${2}_${1}.md",
            &PermissionChecker::disabled(),
        );

        assert_eq!(plan.rows[0].after, "2026_report.md");
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.can_execute());
        Ok(())
    }

    #[test]
    fn partial_match_ranges_are_recorded() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("plan-match-ranges")?;
        let source = directory.path().join("prefix-match-suffix");
        fs::write(&source, b"contents")?;
        let entries = vec![entry(source, EntryKind::File)];

        let plan = build(
            &entries,
            SelectionFilter::Both,
            "match",
            "renamed",
            &PermissionChecker::disabled(),
        );

        assert_eq!(plan.rows[0].match_ranges, vec![7..12]);
        Ok(())
    }

    #[test]
    fn duplicate_destinations_are_conflicts() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("plan-duplicates")?;
        let first = directory.path().join("photo-1.jpg");
        let second = directory.path().join("photo-2.jpg");
        fs::write(&first, b"one")?;
        fs::write(&second, b"two")?;
        let entries = vec![
            entry(first, EntryKind::File),
            entry(second, EntryKind::File),
        ];

        let plan = build(
            &entries,
            SelectionFilter::Both,
            r"-\d",
            "",
            &PermissionChecker::disabled(),
        );

        assert_eq!(plan.conflict_count(), 2);
        assert!(!plan.can_execute());
        assert!(plan.rows.iter().all(|row| {
            row.issues
                .iter()
                .any(|issue| issue.level == IssueLevel::Conflict)
        }));
        assert!(plan.blocking_details().iter().any(|detail| {
            detail.contains("photo-1.jpg -> photo.jpg")
                && detail.contains("CONFLICT: multiple entries would have the same destination")
        }));
        Ok(())
    }

    #[test]
    fn an_unchanged_existing_entry_blocks_a_destination() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("plan-occupied")?;
        let occupied = directory.path().join("name");
        let moving = directory.path().join("name1");
        fs::write(&occupied, b"occupied")?;
        fs::write(&moving, b"moving")?;
        let entries = vec![
            entry(occupied, EntryKind::File),
            entry(moving, EntryKind::File),
        ];

        let plan = build(
            &entries,
            SelectionFilter::Both,
            "1$",
            "",
            &PermissionChecker::disabled(),
        );

        assert_eq!(plan.conflict_count(), 1);
        assert!(!plan.can_execute());
        Ok(())
    }

    #[test]
    fn rename_cycles_are_conflicts() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("plan-swap")?;
        let first = directory.path().join("ab");
        let second = directory.path().join("ba");
        fs::write(&first, b"first")?;
        fs::write(&second, b"second")?;
        let entries = vec![
            entry(first, EntryKind::File),
            entry(second, EntryKind::File),
        ];

        let plan = build(
            &entries,
            SelectionFilter::Both,
            r"^(.)(.)$",
            "$2$1",
            &PermissionChecker::disabled(),
        );

        assert_eq!(plan.conflict_count(), 2);
        assert_eq!(plan.actions.len(), 2);
        assert!(!plan.can_execute());
        assert!(plan.rows.iter().all(|row| {
            row.issues
                .iter()
                .any(|issue| issue.message.contains("cycle"))
        }));
        Ok(())
    }

    #[test]
    fn rename_chains_are_not_conflicts() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("plan-chain")?;
        let first = directory.path().join("a");
        let second = directory.path().join("aa");
        fs::write(&first, b"first")?;
        fs::write(&second, b"second")?;
        let entries = vec![
            entry(first, EntryKind::File),
            entry(second, EntryKind::File),
        ];

        let plan = build(
            &entries,
            SelectionFilter::Both,
            "^",
            "a",
            &PermissionChecker::disabled(),
        );

        assert_eq!(plan.conflict_count(), 0);
        assert_eq!(plan.actions.len(), 2);
        assert!(plan.can_execute());
        Ok(())
    }

    #[test]
    fn filters_select_files_or_directories() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("plan-filters")?;
        let file = directory.path().join("file");
        let folder = directory.path().join("folder");
        fs::write(&file, b"file")?;
        fs::create_dir(&folder)?;
        let entries = vec![
            entry(file.clone(), EntryKind::File),
            entry(folder.clone(), EntryKind::Directory),
        ];

        let files = build(
            &entries,
            SelectionFilter::Files,
            "^",
            "new-",
            &PermissionChecker::disabled(),
        );
        let directories = build(
            &entries,
            SelectionFilter::Directories,
            "^",
            "new-",
            &PermissionChecker::disabled(),
        );

        assert_eq!(files.actions.len(), 1);
        assert_eq!(directories.actions.len(), 1);
        assert_eq!(files.actions[0].source, file);
        assert_eq!(directories.actions[0].source, folder);
        Ok(())
    }

    #[test]
    fn invalid_regular_expressions_block_confirmation() {
        let entries = vec![entry(PathBuf::from("/tmp/file"), EntryKind::File)];
        let plan = build(
            &entries,
            SelectionFilter::Both,
            "(",
            "replacement",
            &PermissionChecker::disabled(),
        );

        assert!(plan.regex_error.is_some());
        assert!(!plan.can_execute());
    }

    #[test]
    fn path_separators_in_replacements_are_conflicts() {
        let entries = vec![entry(PathBuf::from("/tmp/file"), EntryKind::File)];
        let plan = build(
            &entries,
            SelectionFilter::Both,
            "file",
            "nested/file",
            &PermissionChecker::disabled(),
        );

        assert_eq!(plan.conflict_count(), 1);
        assert!(!plan.can_execute());
    }

    #[test]
    fn nested_selected_directories_are_conflicts() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("plan-nested")?;
        let parent = directory.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child)?;
        let entries = vec![
            entry(parent, EntryKind::Directory),
            entry(child, EntryKind::Directory),
        ];

        let plan = build(
            &entries,
            SelectionFilter::Both,
            "^",
            "new-",
            &PermissionChecker::disabled(),
        );

        assert_eq!(plan.conflict_count(), 2);
        assert!(!plan.can_execute());
        Ok(())
    }

    #[test]
    fn a_source_removed_after_loading_is_a_conflict() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("plan-missing-source")?;
        let source = directory.path().join("source");
        fs::write(&source, b"contents")?;
        let entries = vec![entry(source.clone(), EntryKind::File)];
        fs::remove_file(source)?;

        let plan = build(
            &entries,
            SelectionFilter::Both,
            "source$",
            "renamed",
            &PermissionChecker::disabled(),
        );

        assert_eq!(plan.conflict_count(), 1);
        assert!(!plan.can_execute());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_hard_link_is_not_treated_as_the_moving_source() -> Result<(), Box<dyn Error>> {
        let directory = crate::test_support::TempDir::new("plan-hard-link")?;
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        fs::write(&source, b"contents")?;
        fs::hard_link(&source, &destination)?;
        let entries = vec![entry(source, EntryKind::File)];

        let plan = build(
            &entries,
            SelectionFilter::Both,
            "source$",
            "destination",
            &PermissionChecker::disabled(),
        );

        assert_eq!(plan.conflict_count(), 1);
        assert!(!plan.can_execute());
        Ok(())
    }
}
