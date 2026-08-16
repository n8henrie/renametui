use std::process::ExitCode;

fn main() -> ExitCode {
    renametui::run(std::env::args_os())
}
