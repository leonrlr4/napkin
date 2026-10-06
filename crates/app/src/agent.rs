//! Opening or focusing the terminal Ctrl+K runs a Claude Code session in, rooted at the
//! canvas's napkin folder.

use std::path::{Path, PathBuf};
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

/// Claude Code's per-project conversation directory for `folder`, under `config` (its config
/// directory: `$CLAUDE_CONFIG_DIR` if set, else `$HOME/.claude`). Claude Code encodes the
/// absolute folder path by replacing every character that is not ASCII alphanumeric with `-`.
pub fn conversation_dir(config: &Path, folder: &Path) -> PathBuf {
    let encoded: String = folder
        .display()
        .to_string()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    config.join("projects").join(encoded)
}

/// Whether `dir` contains at least one `*.jsonl` conversation file. A missing directory counts
/// as no conversation.
pub fn has_conversation(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries
        .flatten()
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
}

/// `xdg-terminal-exec` and its arguments to open Claude Code in `home`'s napkin folder.
/// `continue_conversation` should be true only when a previous conversation exists for that
/// folder; `claude --continue` prints an error and exits immediately when there is none, so it
/// must be omitted in that case rather than passed unconditionally.
pub fn launch_command(home: &Path, continue_conversation: bool) -> (String, Vec<String>) {
    let napkin_dir = home.join("Documents").join("napkin");
    let mut args = vec![
        format!("--app-id={APP_ID}"),
        format!("--dir={}", napkin_dir.display()),
        "--".to_string(),
        "claude".to_string(),
    ];
    if continue_conversation {
        args.push("--continue".to_string());
    }
    args.push("--dangerously-skip-permissions".to_string());
    args.push("--model".to_string());
    args.push("sonnet".to_string());
    ("xdg-terminal-exec".to_string(), args)
}

/// Focuses an already-open agent terminal, or launches a new one in `home`'s napkin folder
/// (creating it first). A `hyprctl clients -j` that fails to run is treated as no window open,
/// so a launch is attempted instead.
pub fn open(home: &Path) -> Result<(), String> {
    let napkin_dir = home.join("Documents").join("napkin");
    std::fs::create_dir_all(&napkin_dir).map_err(|error| error.to_string())?;

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
            let config = std::env::var_os("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude"));
            let continue_conversation = has_conversation(&conversation_dir(&config, &napkin_dir));
            let (program, args) = launch_command(home, continue_conversation);
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
    fn launches_claude_in_the_napkin_folder_continuing_a_conversation() {
        let (program, args) = launch_command(Path::new("/home/leon"), true);
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

    #[test]
    fn launches_claude_in_the_napkin_folder_without_continuing() {
        let (program, args) = launch_command(Path::new("/home/leon"), false);
        assert_eq!(program, "xdg-terminal-exec");
        assert_eq!(
            args,
            [
                "--app-id=org.napkin.agent",
                "--dir=/home/leon/Documents/napkin",
                "--",
                "claude",
                "--dangerously-skip-permissions",
                "--model",
                "sonnet",
            ]
        );
    }

    #[test]
    fn encodes_the_folder_path_like_claude_code_does() {
        let dir = conversation_dir(
            Path::new("/home/leon/.claude"),
            Path::new("/home/leon/Documents/napkin"),
        );
        assert_eq!(
            dir,
            Path::new("/home/leon/.claude/projects/-home-leon-Documents-napkin")
        );
    }

    #[test]
    fn has_conversation_is_false_for_a_missing_directory() {
        let dir =
            std::env::temp_dir().join(format!("napkin-agent-test-missing-{}", std::process::id()));
        assert!(!has_conversation(&dir));
    }

    #[test]
    fn has_conversation_is_false_for_an_empty_directory_and_true_once_a_jsonl_file_exists() {
        let dir = std::env::temp_dir().join(format!("napkin-agent-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");

        assert!(!has_conversation(&dir));

        std::fs::write(dir.join("session.jsonl"), "{}").expect("write conversation file");
        assert!(has_conversation(&dir));

        std::fs::remove_dir_all(&dir).expect("clean up temp dir");
    }
}
