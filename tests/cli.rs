use std::{error::Error, process::Command};

#[test]
fn help_describes_the_interface() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_renametui"))
        .arg("--help")
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("renametui [--] [PATH ...]"));
    assert!(stdout.contains("F1"));
    assert!(stdout.contains("Ctrl-A"));
    assert!(stdout.contains("Ctrl-E"));
    assert!(stdout.contains("Ctrl-R"));
    Ok(())
}

#[test]
fn version_is_available() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_renametui"))
        .arg("--version")
        .output()?;

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout)?.trim(), "renametui 0.1.0");
    Ok(())
}

#[test]
fn unknown_options_fail_without_entering_the_tui() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_renametui"))
        .arg("--definitely-not-an-option")
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("unknown option"));
    assert!(stderr.contains("--help"));
    Ok(())
}
