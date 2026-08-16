use crate::{
    entry::{Entry, EntryKind},
    input::TextInput,
    permissions::PermissionChecker,
    plan::{build, Plan, RenameAction, SelectionFilter},
    transaction,
    ui,
};
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    DefaultTerminal,
};
use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Focus {
    Pattern,
    Replacement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MessageLevel {
    Info,
    Success,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StatusMessage {
    pub(crate) level: MessageLevel,
    pub(crate) text: String,
}

pub(crate) struct App {
    pub(crate) entries: Vec<Entry>,
    pub(crate) filter: SelectionFilter,
    pub(crate) pattern: TextInput,
    pub(crate) replacement: TextInput,
    pub(crate) focus: Focus,
    pub(crate) plan: Plan,
    pub(crate) scroll: usize,
    pub(crate) confirming: bool,
    pub(crate) showing_blockers: bool,
    pub(crate) message: Option<StatusMessage>,
    permissions: PermissionChecker,
    should_quit: bool,
}

impl App {
    pub(crate) fn new(entries: Vec<Entry>) -> Self {
        let permissions = PermissionChecker::detect();
        let filter = SelectionFilter::Both;
        let pattern = TextInput::default();
        let replacement = TextInput::default();
        let plan = build(
            &entries,
            filter,
            pattern.value(),
            replacement.value(),
            &permissions,
        );

        Self {
            entries,
            filter,
            pattern,
            replacement,
            focus: Focus::Pattern,
            plan,
            scroll: 0,
            confirming: false,
            showing_blockers: false,
            message: None,
            permissions,
            should_quit: false,
        }
    }

    pub(crate) fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| ui::render(frame, self))?;
            match event::read()? {
                Event::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    self.handle_key(key);
                }
                Event::Paste(text) => self.insert_pasted_text(&text),
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.showing_blockers {
            self.handle_blocking_details_key(key);
            return;
        }

        if self.confirming {
            self.handle_confirmation_key(key);
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c' | 'q' | 'C' | 'Q') => self.should_quit = true,
                KeyCode::Char('r' | 'R') => self.request_confirmation(),
                KeyCode::Char('a' | 'A') => self.active_input_mut().home(),
                KeyCode::Char('e' | 'E') => self.active_input_mut().end(),
                KeyCode::Char('u' | 'U') => {
                    self.active_input_mut().clear();
                    self.inputs_changed();
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::F(1) => self.set_filter(SelectionFilter::Files),
            KeyCode::F(2) => self.set_filter(SelectionFilter::Directories),
            KeyCode::F(3) => self.set_filter(SelectionFilter::Both),
            KeyCode::Tab | KeyCode::BackTab => self.toggle_focus(),
            KeyCode::Enter => {
                if self.focus == Focus::Pattern {
                    self.focus = Focus::Replacement;
                } else {
                    self.request_confirmation();
                }
            }
            KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down => self.scroll_by(1),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::PageDown => self.scroll_by(10),
            KeyCode::Home => self.active_input_mut().home(),
            KeyCode::End => self.active_input_mut().end(),
            KeyCode::Left => self.active_input_mut().move_left(),
            KeyCode::Right => self.active_input_mut().move_right(),
            KeyCode::Backspace => {
                if self.active_input_mut().backspace() {
                    self.inputs_changed();
                }
            }
            KeyCode::Delete => {
                if self.active_input_mut().delete() {
                    self.inputs_changed();
                }
            }
            KeyCode::Char(character) if !character.is_control() => {
                self.active_input_mut().insert(character);
                self.inputs_changed();
            }
            KeyCode::Esc => self.message = None,
            _ => {}
        };
    }

    fn handle_confirmation_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'q' | 'C' | 'Q'))
        {
            self.should_quit = true;
            return;
        }

        match key.code {
            KeyCode::Char('y' | 'Y') => self.execute_confirmed_plan(),
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                self.confirming = false;
                self.message = Some(StatusMessage {
                    level: MessageLevel::Info,
                    text: "rename cancelled".to_owned(),
                });
            }
            _ => {}
        };
    }

    fn handle_blocking_details_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'q' | 'C' | 'Q'))
        {
            self.should_quit = true;
            return;
        }

        if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
            self.showing_blockers = false;
        }
    }

    fn active_input_mut(&mut self) -> &mut TextInput {
        match self.focus {
            Focus::Pattern => &mut self.pattern,
            Focus::Replacement => &mut self.replacement,
        }
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Pattern => Focus::Replacement,
            Focus::Replacement => Focus::Pattern,
        };
    }

    fn set_filter(&mut self, filter: SelectionFilter) {
        self.filter = filter;
        self.showing_blockers = false;
        self.message = Some(StatusMessage {
            level: MessageLevel::Info,
            text: format!("selection changed to {}", filter.label()),
        });
        self.rebuild_plan();
    }

    fn scroll_by(&mut self, amount: usize) {
        let maximum = self.plan.rows.len().saturating_sub(1);
        self.scroll = self.scroll.saturating_add(amount).min(maximum);
    }

    fn insert_pasted_text(&mut self, text: &str) {
        let sanitized: String = text
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        if sanitized.is_empty() {
            return;
        }
        self.active_input_mut().insert_str(&sanitized);
        self.inputs_changed();
    }

    fn inputs_changed(&mut self) {
        self.showing_blockers = false;
        self.message = None;
        self.rebuild_plan();
    }

    fn rebuild_plan(&mut self) {
        self.plan = build(
            &self.entries,
            self.filter,
            self.pattern.value(),
            self.replacement.value(),
            &self.permissions,
        );
        self.scroll = self
            .scroll
            .min(self.plan.rows.len().saturating_sub(1));
    }

    fn request_confirmation(&mut self) {
        self.rebuild_plan();
        if self.plan.can_execute() {
            self.confirming = true;
            self.showing_blockers = false;
            self.message = None;
        } else {
            self.confirming = false;
            self.showing_blockers = true;
            self.message = None;
        };
    }

    fn execute_confirmed_plan(&mut self) {
        let actions = self.plan.actions.clone();
        match transaction::execute(&actions) {
            Ok(()) => {
                apply_successful_actions(&mut self.entries, &actions);
                self.entries
                    .sort_by(|left, right| left.path.cmp(&right.path));
                self.pattern.clear();
                self.replacement.clear();
                self.focus = Focus::Pattern;
                self.scroll = 0;
                self.confirming = false;
                self.showing_blockers = false;
                self.message = Some(StatusMessage {
                    level: MessageLevel::Success,
                    text: format!(
                        "renamed {} {}",
                        actions.len(),
                        if actions.len() == 1 {
                            "entry"
                        } else {
                            "entries"
                        }
                    ),
                });
                self.rebuild_plan();
            }
            Err(error) => {
                self.confirming = false;
                self.showing_blockers = false;
                self.message = Some(StatusMessage {
                    level: MessageLevel::Error,
                    text: error.to_string(),
                });
                self.rebuild_plan();
            }
        };
    }
}

fn apply_successful_actions(entries: &mut [Entry], actions: &[RenameAction]) {
    let directory_actions: Vec<&RenameAction> = actions
        .iter()
        .filter(|action| {
            entries.iter().any(|entry| {
                entry.kind == EntryKind::Directory && entry.path == action.source
            })
        })
        .collect();

    for entry in entries {
        let original = entry.path.clone();
        if let Some(action) = actions.iter().find(|action| action.source == original) {
            entry.path.clone_from(&action.destination);
            continue;
        }

        let containing_action = directory_actions
            .iter()
            .copied()
            .filter(|action| original.starts_with(&action.source))
            .max_by_key(|action| action.source.components().count());
        let Some(action) = containing_action else {
            continue;
        };
        let Ok(suffix) = original.strip_prefix(&action.source) else {
            continue;
        };
        entry.path = action.destination.join(suffix);
    };
}

#[cfg(test)]
mod tests {
    use super::{apply_successful_actions, App, Focus};
    use crate::{
        entry::{Entry, EntryKind},
        plan::RenameAction,
    };
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn file(path: &str) -> Entry {
        Entry {
            path: PathBuf::from(path),
            kind: EntryKind::File,
        }
    }

    #[test]
    fn successful_chain_updates_each_original_entry_once() {
        let mut entries = vec![file("/tmp/a"), file("/tmp/aa")];
        let actions = vec![
            RenameAction {
                source: PathBuf::from("/tmp/a"),
                destination: PathBuf::from("/tmp/aa"),
            },
            RenameAction {
                source: PathBuf::from("/tmp/aa"),
                destination: PathBuf::from("/tmp/aaa"),
            },
        ];

        apply_successful_actions(&mut entries, &actions);

        assert_eq!(entries[0].path, PathBuf::from("/tmp/aa"));
        assert_eq!(entries[1].path, PathBuf::from("/tmp/aaa"));
    }

    #[test]
    fn renaming_a_directory_updates_listed_descendants() {
        let mut entries = vec![
            Entry {
                path: PathBuf::from("/tmp/old"),
                kind: EntryKind::Directory,
            },
            file("/tmp/old/child"),
        ];
        let actions = vec![RenameAction {
            source: PathBuf::from("/tmp/old"),
            destination: PathBuf::from("/tmp/new"),
        }];

        apply_successful_actions(&mut entries, &actions);

        assert_eq!(entries[0].path, PathBuf::from("/tmp/new"));
        assert_eq!(entries[1].path, PathBuf::from("/tmp/new/child"));
    }

    #[test]
    fn control_a_moves_the_active_input_to_the_start() {
        let mut app = App::new(Vec::new());
        app.pattern.insert_str("alpha");

        app.handle_key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL,
        ));

        assert_eq!(app.focus, Focus::Pattern);
        assert_eq!(app.pattern.cursor(), 0);
    }

    #[test]
    fn control_e_moves_the_active_input_to_the_end() {
        let mut app = App::new(Vec::new());
        app.replacement.insert_str("replacement");
        app.replacement.home();
        app.focus = Focus::Replacement;

        app.handle_key(KeyEvent::new(
            KeyCode::Char('e'),
            KeyModifiers::CONTROL,
        ));

        assert_eq!(app.replacement.cursor(), app.replacement.value().len());
    }

    #[test]
    fn invalid_submission_opens_the_blocking_details() {
        let mut app = App::new(Vec::new());
        app.focus = Focus::Replacement;

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(app.showing_blockers);
        assert!(!app.confirming);
    }
}
