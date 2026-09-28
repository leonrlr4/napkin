//! Command line parsing for the `napkin` binary: opening the GUI on a file, or one of the
//! control subcommands that talk to an already-running napkin over its Unix socket.

use std::path::PathBuf;

use crate::control::RenderTarget;

pub const USAGE: &str = "usage: napkin [FILE.excalidraw] [--bench]
       napkin status
       napkin scene [--full]
       napkin selection [--full]
       napkin view
       napkin apply                 (reads {\"ops\": [...]} from stdin)
       napkin render --out FILE.png [--selection | --view]";

#[derive(Debug, PartialEq)]
pub enum Command {
    Gui { file: Option<PathBuf>, bench: bool },
    Control(ControlCommand),
}

#[derive(Debug, PartialEq)]
pub enum ControlCommand {
    Status,
    Scene { full: bool },
    Selection { full: bool },
    View,
    Apply,
    Render { out: PathBuf, target: RenderTarget },
}

/// Arguments after the program name. The first argument opens a control subcommand when it is
/// exactly one of the names below; anything else, including no arguments, opens the GUI, so a
/// file actually named e.g. `status` needs to be passed as `./status`.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut args = args.into_iter().peekable();
    match args.peek().map(String::as_str) {
        Some("status") => {
            args.next();
            no_more_args(args)?;
            Ok(Command::Control(ControlCommand::Status))
        }
        Some("scene") => {
            args.next();
            let full = full_flag(args)?;
            Ok(Command::Control(ControlCommand::Scene { full }))
        }
        Some("selection") => {
            args.next();
            let full = full_flag(args)?;
            Ok(Command::Control(ControlCommand::Selection { full }))
        }
        Some("view") => {
            args.next();
            no_more_args(args)?;
            Ok(Command::Control(ControlCommand::View))
        }
        Some("apply") => {
            args.next();
            no_more_args(args)?;
            Ok(Command::Control(ControlCommand::Apply))
        }
        Some("render") => {
            args.next();
            parse_render(args).map(Command::Control)
        }
        _ => parse_gui(args),
    }
}

/// No `--full`, or exactly one `--full` and nothing after it.
fn full_flag(mut args: impl Iterator<Item = String>) -> Result<bool, String> {
    match args.next() {
        None => Ok(false),
        Some(flag) if flag == "--full" => {
            no_more_args(args)?;
            Ok(true)
        }
        Some(other) => Err(format!("unknown argument: {other}")),
    }
}

fn no_more_args(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    match args.next() {
        None => Ok(()),
        Some(arg) => Err(format!("unexpected extra argument: {arg}")),
    }
}

fn parse_render(args: impl Iterator<Item = String>) -> Result<ControlCommand, String> {
    let mut out = None;
    let mut target = None;
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                let value = args.next().ok_or("--out needs a path")?;
                out = Some(PathBuf::from(value));
            }
            "--selection" if target.is_none() => target = Some(RenderTarget::Selection),
            "--view" if target.is_none() => target = Some(RenderTarget::View),
            "--selection" | "--view" => {
                return Err("--selection and --view are mutually exclusive".to_string());
            }
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    let out = out.ok_or("--out is required")?;
    let out = if out.is_relative() {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(out)
    } else {
        out
    };
    Ok(ControlCommand::Render {
        out,
        target: target.unwrap_or(RenderTarget::All),
    })
}

fn parse_gui(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut file = None;
    let mut bench = false;
    for arg in args {
        if arg == "--bench" {
            bench = true;
        } else if let Some(flag) = arg.strip_prefix('-') {
            return Err(format!("unknown flag: -{flag}"));
        } else if file.is_some() {
            return Err(format!("unexpected extra argument: {arg}"));
        } else {
            file = Some(PathBuf::from(arg));
        }
    }
    Ok(Command::Gui { file, bench })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_file_and_bench_flag_in_any_order() {
        assert_eq!(
            parse(args(&[])),
            Ok(Command::Gui {
                file: None,
                bench: false
            })
        );
        assert_eq!(
            parse(args(&["a.excalidraw"])),
            Ok(Command::Gui {
                file: Some("a.excalidraw".into()),
                bench: false
            })
        );
        assert_eq!(
            parse(args(&["--bench", "a.excalidraw"])),
            Ok(Command::Gui {
                file: Some("a.excalidraw".into()),
                bench: true
            })
        );
    }

    #[test]
    fn rejects_unknown_flags_and_extra_files() {
        assert!(parse(args(&["--nope"])).is_err());
        assert!(parse(args(&["a.excalidraw", "b.excalidraw"])).is_err());
    }

    #[test]
    fn parses_control_subcommands() {
        assert_eq!(
            parse(args(&["status"])),
            Ok(Command::Control(ControlCommand::Status))
        );
        assert_eq!(
            parse(args(&["scene", "--full"])),
            Ok(Command::Control(ControlCommand::Scene { full: true }))
        );
        assert_eq!(
            parse(args(&["apply"])),
            Ok(Command::Control(ControlCommand::Apply))
        );
        assert_eq!(
            parse(args(&["render", "--out", "/tmp/a.png", "--view"])),
            Ok(Command::Control(ControlCommand::Render {
                out: "/tmp/a.png".into(),
                target: RenderTarget::View
            }))
        );
        assert!(parse(args(&["render"])).is_err(), "--out is required");
        assert!(parse(args(&["render", "--out", "a.png", "--view", "--selection"])).is_err());
        assert!(parse(args(&["scene", "extra"])).is_err());
        assert_eq!(
            parse(args(&["./status"])),
            Ok(Command::Gui {
                file: Some("./status".into()),
                bench: false
            })
        );
    }
}
