use nptk::std::path::Path;
use nptk::std::process::{Command, Stdio};

fn spawn_in_dir(program: &str, args: &[&str], directory: &Path) -> Result<(), String> {
    Command::new(program)
        .args(args)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Failed to start {program}: {error}"))?;
    Ok(())
}

pub fn open_terminal_in_directory(
    directory: &Path,
    config_command: Option<&str>,
) -> Result<(), String> {
    if !directory.is_dir() {
        return Err("Not a directory".to_string());
    }

    if let Ok(terminal) = std::env::var("TERMINAL") {
        let parts: Vec<&str> = terminal.split_whitespace().collect();
        if let Some(program) = parts.first() {
            let arguments: Vec<&str> = parts.iter().skip(1).copied().collect();
            return spawn_in_dir(program, &arguments, directory);
        }
    }

    if let Some(command) = config_command {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if let Some(program) = parts.first() {
            let arguments: Vec<&str> = parts.iter().skip(1).copied().collect();
            if spawn_in_dir(program, &arguments, directory).is_ok() {
                return Ok(());
            }
        }
    }

    if spawn_in_dir("x-terminal-emulator", &[], directory).is_ok() {
        return Ok(());
    }

    let fallbacks: &[(&str, &[&str])] = &[
        ("kitty", &["--directory"]),
        ("alacritty", &["--working-directory"]),
        ("konsole", &["--workdir"]),
        ("gnome-terminal", &["--working-directory"]),
    ];
    for (program, flag) in fallbacks {
        let directory_string = directory.to_string_lossy();
        let arguments = vec![flag[0], directory_string.as_ref()];
        if spawn_in_dir(program, &arguments, directory).is_ok() {
            return Ok(());
        }
    }

    Err(
        "No terminal emulator found (set TERMINAL or install x-terminal-emulator / kitty / alacritty / konsole / gnome-terminal)".to_string(),
    )
}
