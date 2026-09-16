//! Command line parsing for the `napkin` binary.

pub const USAGE: &str = "usage: napkin [FILE.excalidraw] [--bench]";

#[derive(Debug, PartialEq)]
pub struct Cli {
    pub file: Option<std::path::PathBuf>,
    pub bench: bool,
}

/// Arguments after the program name.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Cli, String> {
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
            file = Some(std::path::PathBuf::from(arg));
        }
    }
    Ok(Cli { file, bench })
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
            Ok(Cli {
                file: None,
                bench: false
            })
        );
        assert_eq!(
            parse(args(&["a.excalidraw"])),
            Ok(Cli {
                file: Some("a.excalidraw".into()),
                bench: false
            })
        );
        assert_eq!(
            parse(args(&["--bench", "a.excalidraw"])),
            Ok(Cli {
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
}
