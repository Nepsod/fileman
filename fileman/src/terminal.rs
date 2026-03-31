//! Spawn a terminal emulator with a working directory.

use std::path::Path;
use std::process::{Command, Stdio};

fn spawn_in_dir(program: &str, args: &[&str], dir: &Path) -> Result<(), String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn()
        .map_err(|e| format!("Failed to start {}: {}", program, e))?;
    Ok(())
}

/// Open a terminal window whose initial working directory is `dir`.
pub fn open_terminal_in_directory(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err("Not a directory".to_string());
    }

    if let Ok(term) = std::env::var("TERMINAL") {
        let parts: Vec<&str> = term.split_whitespace().collect();
        if let Some(prog) = parts.first() {
            let rest: Vec<&str> = parts.iter().skip(1).copied().collect();
            return spawn_in_dir(prog, &rest, dir);
        }
    }

    if spawn_in_dir("x-terminal-emulator", &[], dir).is_ok() {
        return Ok(());
    }

    let fallbacks: &[(&str, &[&str])] = &[
        ("kitty", &["--directory"]),
        ("alacritty", &["--working-directory"]),
        ("konsole", &["--workdir"]),
        ("gnome-terminal", &["--working-directory"]),
    ];
    for (prog, flag) in fallbacks {
        let dir_str = dir.to_string_lossy();
        let args = vec![flag[0], dir_str.as_ref()];
        if spawn_in_dir(prog, &args, dir).is_ok() {
            return Ok(());
        }
    }

    Err("No terminal emulator found (set TERMINAL or install x-terminal-emulator / kitty / alacritty / konsole / gnome-terminal)".to_string())
}
