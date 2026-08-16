use std::{ffi::OsString, path::PathBuf};

pub(crate) const HELP: &str = concat!(
    "renametui ",
    env!("CARGO_PKG_VERSION"),
    "\nA conflict-aware regex renamer for files and directories.\n\n",
    "USAGE:\n",
    "    renametui [--] [PATH ...]\n\n",
    "PATHS:\n",
    "    Explicit files or directories to consider.\n",
    "    When no paths are supplied, every immediate entry in the current directory is used.\n",
    "    Directories are not traversed.\n\n",
    "KEYS:\n",
    "    Tab          Switch between pattern and replacement inputs\n",
    "    Enter        Advance to replacement, then review the current plan\n",
    "    F1           Select files and other non-directory entries\n",
    "    F2           Select directories\n",
    "    F3           Select both\n",
    "    Ctrl-A       Move to the start of the active input\n",
    "    Ctrl-E       Move to the end of the active input\n",
    "    Ctrl-R       Review the current plan from either input\n",
    "    y            Confirm while the confirmation dialog is open\n",
    "    n / Esc      Cancel the confirmation dialog\n",
    "    Ctrl-Q       Quit\n\n",
    "NOTES:\n",
    "    The pattern is a Rust regular expression.\n",
    "    The replacement supports captures such as $1 and ${name}.\n",
    "    Only each entry's final path component is changed.\n",
);

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum CliCommand {
    Help,
    Version,
    Run(Vec<PathBuf>),
}

pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<CliCommand, String> {
    let mut arguments = args.into_iter();
    let _ = arguments.next();
    let mut parse_options = true;
    let mut paths = Vec::new();

    for argument in arguments {
        if parse_options {
            match argument.to_str() {
                Some("--") => {
                    parse_options = false;
                    continue;
                }
                Some("-h" | "--help") => return Ok(CliCommand::Help),
                Some("-V" | "--version") => return Ok(CliCommand::Version),
                Some(option) if option.starts_with('-') && option != "-" => {
                    return Err(format!(
                        "unknown option: {option}\nRun 'renametui --help' for usage."
                    ));
                }
                _ => {}
            }
        }

        paths.push(PathBuf::from(argument));
    }

    Ok(CliCommand::Run(paths))
}

#[cfg(test)]
mod tests {
    use super::{parse, CliCommand};
    use std::{ffi::OsString, path::PathBuf};

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().copied().map(OsString::from).collect()
    }

    #[test]
    fn no_paths_selects_the_default_mode() {
        let parsed = parse(arguments(&["renametui"]));
        assert_eq!(parsed, Ok(CliCommand::Run(Vec::new())));
    }

    #[test]
    fn paths_are_preserved() {
        let parsed = parse(arguments(&["renametui", "one", "two"]));
        assert_eq!(
            parsed,
            Ok(CliCommand::Run(vec![
                PathBuf::from("one"),
                PathBuf::from("two")
            ]))
        );
    }

    #[test]
    fn option_terminator_allows_dash_prefixed_paths() {
        let parsed = parse(arguments(&["renametui", "--", "--literal"]));
        assert_eq!(
            parsed,
            Ok(CliCommand::Run(vec![PathBuf::from("--literal")]))
        );
    }

    #[test]
    fn unknown_options_are_rejected() {
        let parsed = parse(arguments(&["renametui", "--unknown"]));
        assert!(parsed.is_err());
    }
}
