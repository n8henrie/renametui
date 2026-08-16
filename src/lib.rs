#![forbid(unsafe_code)]

mod app;
mod cli;
mod entry;
mod fsutil;
mod input;
mod permissions;
mod plan;
mod rename_order;
mod transaction;
mod ui;

use app::App;
use cli::CliCommand;
use std::{
    ffi::OsString,
    io::{self, Write},
    process::ExitCode,
};

/// Parses command-line arguments and runs the terminal application.
#[must_use]
pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    match cli::parse(args) {
        Ok(CliCommand::Help) => write_standard_output(cli::HELP),
        Ok(CliCommand::Version) => write_standard_output(concat!(
            "renametui ",
            env!("CARGO_PKG_VERSION"),
            "\n"
        )),
        Ok(CliCommand::Run(paths)) => run_application(paths),
        Err(error) => report_error(&error, ExitCode::from(2)),
    }
}

fn run_application(paths: Vec<std::path::PathBuf>) -> ExitCode {
    let entries = match entry::load(paths) {
        Ok(entries) => entries,
        Err(error) => return report_error(&error.to_string(), ExitCode::FAILURE),
    };
    let mut app = App::new(entries);
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => {
            return report_error(
                &format!("terminal initialization failed: {error}"),
                ExitCode::FAILURE,
            );
        }
    };
    let run_result = app.run(&mut terminal);
    let restore_result = ratatui::try_restore();

    match (run_result, restore_result) {
        (Ok(()), Ok(())) => ExitCode::SUCCESS,
        (Err(run_error), Ok(())) => report_error(
            &format!("terminal session failed: {run_error}"),
            ExitCode::FAILURE,
        ),
        (Ok(()), Err(restore_error)) => report_error(
            &format!("terminal restoration failed: {restore_error}"),
            ExitCode::FAILURE,
        ),
        (Err(run_error), Err(restore_error)) => report_error(
            &format!(
                "terminal session failed: {run_error}; restoration also failed: {restore_error}"
            ),
            ExitCode::FAILURE,
        ),
    }
}

fn write_standard_output(text: &str) -> ExitCode {
    match write_to_standard_output(text) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => report_error(
            &format!("could not write output: {error}"),
            ExitCode::FAILURE,
        ),
    }
}

fn write_to_standard_output(text: &str) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(text.as_bytes())?;
    handle.flush()
}

fn report_error(message: &str, exit_code: ExitCode) -> ExitCode {
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    let _ignored = writeln!(handle, "renametui: {message}");
    exit_code
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::{fs, io, path::Path, path::PathBuf};

    pub(crate) struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        pub(crate) fn new(label: &str) -> io::Result<Self> {
            let root = std::env::temp_dir();
            for attempt in 0_u32..10_000 {
                let path = root.join(format!(
                    "renametui-{label}-{}-{attempt}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Ok(Self { path }),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
            }

            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "could not allocate a temporary test directory",
            ))
        }

        pub(crate) fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.path);
        }
    }
}
