//! Opening or focusing the terminal Ctrl+K runs a Claude Code session in, rooted at the
//! canvas's napkin folder.

use std::path::Path;
use std::process::Command;

/// The app-id `xdg-terminal-exec` launches the agent terminal with, and the id `hyprctl` looks
/// for to tell whether one is already open.
pub const APP_ID: &str = "org.napkin.agent";

/// What Ctrl+K should do: bring an already-open agent terminal to the front, or start one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Focus,
    Launch,
}

/// `Focus` when `clients_json` (the output of `hyprctl clients -j`) lists a window whose class
/// is [`APP_ID`]; `Launch` otherwise, including when `clients_json` fails to parse.
pub fn action(clients_json: &str) -> Action {
    let Ok(clients) = serde_json::from_str::<serde_json::Value>(clients_json) else {
        return Action::Launch;
    };
    let has_agent_window = clients
        .as_array()
        .into_iter()
        .flatten()
        .any(|client| client.get("class").and_then(|class| class.as_str()) == Some(APP_ID));
    if has_agent_window {
        Action::Focus
    } else {
        Action::Launch
    }
}

/// `xdg-terminal-exec` and its arguments to open Claude Code in `home`'s napkin folder.
pub fn launch_command(home: &Path) -> (String, Vec<String>) {
    let napkin_dir = home.join("Documents").join("napkin");
    (
        "xdg-terminal-exec".to_string(),
        vec![
            format!("--app-id={APP_ID}"),
            format!("--dir={}", napkin_dir.display()),
            "--".to_string(),
            "claude".to_string(),
            "--continue".to_string(),
            "--dangerously-skip-permissions".to_string(),
            "--model".to_string(),
            "sonnet".to_string(),
        ],
    )
}

/// Focuses an already-open agent terminal, or launches a new one in `home`'s napkin folder
/// (creating it first). A `hyprctl clients -j` that fails to run is treated as no window open,
/// so a launch is attempted instead.
pub fn open(home: &Path) -> Result<(), String> {
    std::fs::create_dir_all(home.join("Documents").join("napkin"))
        .map_err(|error| error.to_string())?;

    let clients_json = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();

    match action(&clients_json) {
        Action::Focus => {
            Command::new("hyprctl")
                .args([
                    "dispatch",
                    &format!(r#"hl.dsp.focus({{ window = "class:{APP_ID}" }})"#),
                ])
                .status()
                .map_err(|error| error.to_string())?;
            Ok(())
        }
        Action::Launch => {
            let (program, args) = launch_command(home);
            let mut child = Command::new(&program)
                .args(&args)
                .spawn()
                .map_err(|error| format!("{program}: {error}"))?;
            // Reaps the child once the terminal exits, so it never lingers as a zombie process;
            // nothing here needs its exit status.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focuses_an_existing_agent_window() {
        let clients = r#"[{"class": "foot", "pid": 1}, {"class": "org.napkin.agent", "pid": 2}]"#;
        assert_eq!(action(clients), Action::Focus);
        assert_eq!(action(r#"[{"class": "napkin"}]"#), Action::Launch);
        assert_eq!(action("not json"), Action::Launch);
    }

    #[test]
    fn launches_claude_in_the_napkin_folder() {
        let (program, args) = launch_command(Path::new("/home/leon"));
        assert_eq!(program, "xdg-terminal-exec");
        assert_eq!(
            args,
            [
                "--app-id=org.napkin.agent",
                "--dir=/home/leon/Documents/napkin",
                "--",
                "claude",
                "--continue",
                "--dangerously-skip-permissions",
                "--model",
                "sonnet",
            ]
        );
    }
}
