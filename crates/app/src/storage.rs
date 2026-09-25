//! Where canvases live on disk (spec §5.4), loading them, and writing them atomically
//! (spec §5.5).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{fs, io};

/// The directory canvases live in, and the file that remembers which one was last open.
pub struct Paths {
    pub canvases: PathBuf,
    pub last: PathBuf,
}

impl Paths {
    /// `<home>/Documents/napkin` and `<home>/.local/state/napkin/last` (spec §5.4).
    pub fn from_home(home: &Path) -> Paths {
        Paths {
            canvases: home.join("Documents").join("napkin"),
            last: home
                .join(".local")
                .join("state")
                .join("napkin")
                .join("last"),
        }
    }

    /// From `$HOME`; `None` when it is unset or empty.
    pub fn from_env() -> Option<Paths> {
        let home = std::env::var_os("HOME")?;
        if home.is_empty() {
            return None;
        }
        Some(Paths::from_home(Path::new(&home)))
    }

    pub fn scratch(&self) -> PathBuf {
        self.canvases.join("scratch.excalidraw")
    }
}

/// The result of reading and parsing a single file.
#[derive(Debug)]
pub enum Loaded {
    Missing,
    Parsed {
        file: scene::SceneFile,
        mtime: SystemTime,
    },
    /// Unreadable or unparseable; the message names the path.
    Invalid(String),
}

/// Reads and parses `path`. A missing file is [`Loaded::Missing`]; any other I/O error or a
/// parse failure is [`Loaded::Invalid`] with a message naming `path`.
pub fn load(path: &Path) -> Loaded {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Loaded::Missing,
        Err(error) => return Loaded::Invalid(format!("{}: {error}", path.display())),
    };
    match scene::SceneFile::from_json_str(&text) {
        Ok(file) => match modified(path) {
            Ok(Some(mtime)) => Loaded::Parsed { file, mtime },
            // The file vanished between the read above and this stat.
            Ok(None) => Loaded::Missing,
            Err(error) => Loaded::Invalid(format!("{}: {error}", path.display())),
        },
        Err(error) => Loaded::Invalid(format!("{}: {error}", path.display())),
    }
}

/// The content of the canvas the editor opened.
#[derive(Debug)]
pub enum Content {
    /// `mtime` is `None` when the file does not exist yet.
    Editable {
        file: scene::SceneFile,
        mtime: Option<SystemTime>,
    },
    /// Shown, never written (spec §8).
    Unreadable(String),
}

/// The outcome of [`open_at_startup`].
#[derive(Debug)]
pub struct Opened {
    pub path: PathBuf,
    /// The file stem.
    pub name: String,
    pub content: Content,
    /// Why another file than the remembered one opened (spec §8).
    pub notice: Option<String>,
}

fn name_of(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn open_scratch(paths: &Paths, notice: Option<String>) -> Opened {
    let path = paths.scratch();
    let content = match load(&path) {
        Loaded::Parsed { file, mtime } => Content::Editable {
            file,
            mtime: Some(mtime),
        },
        Loaded::Missing => match write_atomic(&path, &scene::SceneFile::new().to_json_string()) {
            Ok(mtime) => Content::Editable {
                file: scene::SceneFile::new(),
                mtime: Some(mtime),
            },
            Err(error) => Content::Unreadable(format!("{}: {error}", path.display())),
        },
        Loaded::Invalid(message) => Content::Unreadable(message),
    };
    Opened {
        name: name_of(&path),
        path,
        content,
        notice,
    }
}

/// The path remembered in `last`, or `None` when it is unreadable or blank.
fn read_last(last: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(last).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Spec §5.4 and §8: the requested file (even if missing or unparseable), else the file in
/// `last` if it parses, else `scratch`, created when missing. Writes nothing else.
pub fn open_at_startup(paths: &Paths, requested: Option<&Path>) -> Opened {
    if let Some(requested) = requested {
        let content = match load(requested) {
            Loaded::Missing => Content::Editable {
                file: scene::SceneFile::new(),
                mtime: None,
            },
            Loaded::Parsed { file, mtime } => Content::Editable {
                file,
                mtime: Some(mtime),
            },
            Loaded::Invalid(message) => Content::Unreadable(message),
        };
        return Opened {
            name: name_of(requested),
            path: requested.to_path_buf(),
            content,
            notice: None,
        };
    }

    let Some(target) = read_last(&paths.last) else {
        return open_scratch(paths, None);
    };
    match load(&target) {
        Loaded::Parsed { file, mtime } => Opened {
            name: name_of(&target),
            path: target,
            content: Content::Editable {
                file,
                mtime: Some(mtime),
            },
            notice: None,
        },
        Loaded::Missing => open_scratch(paths, None),
        Loaded::Invalid(message) => open_scratch(paths, Some(message)),
    }
}

/// Stores `path` (made absolute) in `last`, creating its directory.
pub fn remember(paths: &Paths, path: &Path) -> io::Result<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if let Some(parent) = paths.last.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&paths.last, absolute.to_string_lossy().as_bytes())
}

/// `path` with every symlink component resolved, including a symlink `path` itself: the real
/// file [`write_atomic`] must rename over, so that renaming a temporary file onto a symlinked
/// target replaces what the link points at instead of replacing the link itself with a plain
/// file. Falls back to `path` unresolved when it (or a parent directory) does not exist yet,
/// or is not a symlink, or resolving it fails for any other reason: `fs::canonicalize` needs
/// every component to exist, which a target being written for the first time might not.
fn resolve_target(path: &Path) -> PathBuf {
    if let Ok(real) = fs::canonicalize(path) {
        return real;
    }
    let Some(file_name) = path.file_name() else {
        return path.to_path_buf();
    };
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    match fs::canonicalize(dir) {
        Ok(real_dir) => real_dir.join(file_name),
        Err(_) => path.to_path_buf(),
    }
}

/// Writes a sibling temporary file, syncs it, renames it over `path` and syncs the directory
/// (spec §5.5). Creates missing parent directories; removes the temporary file on failure.
///
/// A symlinked `path` is resolved to its real target first ([`resolve_target`]), so the rename
/// replaces that file's contents rather than replacing the symlink itself with a plain file.
/// The temporary file starts with the target's existing permission bits, when it has any, so a
/// mode set on the file (spec §5.4 says nothing about it, but a hand-`chmod`ed canvas should
/// keep it) survives the replacement instead of falling back to the process's umask default.
/// The temporary file's name includes this process's id, so two napkin processes writing the
/// same path concurrently never share (and race on) one temporary file.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<SystemTime> {
    let create_dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(create_dir)?;

    let target = resolve_target(path);
    let dir = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(file_name);
    tmp_name.push(format!(".{}.napkin-tmp", std::process::id()));
    let tmp_path = dir.join(tmp_name);

    let permissions = fs::metadata(&target).ok().map(|meta| meta.permissions());

    let result = (|| {
        let mut tmp_file = fs::File::create(&tmp_path)?;
        if let Some(permissions) = &permissions {
            tmp_file.set_permissions(permissions.clone())?;
        }
        tmp_file.write_all(contents.as_bytes())?;
        tmp_file.sync_all()?;
        drop(tmp_file);
        fs::rename(&tmp_path, &target)?;
        fs::File::open(dir)?.sync_all()?;
        modified(&target)?
            .ok_or_else(|| io::Error::other("file vanished right after being written"))
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

/// `None` when the file does not exist.
pub fn modified(path: &Path) -> io::Result<Option<SystemTime>> {
    match fs::metadata(path) {
        Ok(meta) => meta.modified().map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(name: &str) -> TempHome {
            let dir =
                std::env::temp_dir().join(format!("napkin-storage-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempHome(dir)
        }

        fn paths(&self) -> Paths {
            Paths::from_home(&self.0)
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const VALID: &str = r#"{"type":"excalidraw","version":2,"elements":[],"appState":{}}"#;

    #[test]
    fn paths_follow_the_spec() {
        let paths = Paths::from_home(Path::new("/home/u"));
        assert_eq!(paths.canvases, PathBuf::from("/home/u/Documents/napkin"));
        assert_eq!(
            paths.last,
            PathBuf::from("/home/u/.local/state/napkin/last")
        );
        assert_eq!(
            paths.scratch(),
            PathBuf::from("/home/u/Documents/napkin/scratch.excalidraw")
        );
    }

    #[test]
    fn first_start_creates_scratch() {
        let home = TempHome::new("first");
        let paths = home.paths();
        let opened = open_at_startup(&paths, None);
        assert_eq!(opened.path, paths.scratch());
        assert_eq!(opened.name, "scratch");
        assert!(matches!(
            opened.content,
            Content::Editable { mtime: Some(_), .. }
        ));
        assert!(opened.notice.is_none());
        assert!(paths.scratch().exists());
    }

    #[test]
    fn reopens_the_last_file_and_falls_back_when_it_is_broken() {
        let home = TempHome::new("last");
        let paths = home.paths();
        let good = home.0.join("good.excalidraw");
        std::fs::write(&good, VALID).unwrap();
        remember(&paths, &good).unwrap();
        let opened = open_at_startup(&paths, None);
        assert_eq!(
            (opened.path.as_path(), opened.name.as_str()),
            (good.as_path(), "good")
        );

        std::fs::write(&good, "{ broken").unwrap();
        let opened = open_at_startup(&paths, None);
        assert_eq!(opened.path, paths.scratch());
        let notice = opened.notice.expect("a notice about the broken file");
        assert!(notice.contains("good.excalidraw"), "{notice}");
        assert_eq!(
            std::fs::read_to_string(&good).unwrap(),
            "{ broken",
            "never overwritten"
        );

        std::fs::remove_file(&good).unwrap();
        let opened = open_at_startup(&paths, None);
        assert_eq!(opened.path, paths.scratch());
        assert!(opened.notice.is_none(), "a vanished file is not an error");
    }

    #[test]
    fn requested_files_open_even_when_missing_or_broken() {
        let home = TempHome::new("requested");
        let paths = home.paths();
        let missing = home.0.join("new.excalidraw");
        let opened = open_at_startup(&paths, Some(missing.as_path()));
        assert!(matches!(
            opened.content,
            Content::Editable { mtime: None, .. }
        ));
        assert!(!missing.exists(), "created on the first save");

        let broken = home.0.join("broken.excalidraw");
        std::fs::write(&broken, "[]").unwrap();
        let opened = open_at_startup(&paths, Some(broken.as_path()));
        assert!(
            matches!(&opened.content, Content::Unreadable(e) if e.contains("broken.excalidraw"))
        );
    }

    #[test]
    fn atomic_write_through_a_symlink_keeps_the_link_and_updates_the_real_file() {
        use std::os::unix::fs::symlink;

        let home = TempHome::new("symlink");
        let real = home.0.join("real.excalidraw");
        std::fs::write(&real, VALID).unwrap();
        let link = home.0.join("link.excalidraw");
        symlink(&real, &link).unwrap();

        write_atomic(&link, "second").unwrap();

        let link_meta = std::fs::symlink_metadata(&link).unwrap();
        assert!(link_meta.file_type().is_symlink(), "still a symlink");
        assert_eq!(std::fs::read_link(&link).unwrap(), real);
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "second");
    }

    #[test]
    fn atomic_write_preserves_the_target_files_permission_bits() {
        use std::os::unix::fs::PermissionsExt;

        let home = TempHome::new("perms");
        let path = home.0.join("mode.excalidraw");
        write_atomic(&path, VALID).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        write_atomic(&path, "second").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{mode:o}");
    }

    #[test]
    fn atomic_writes_replace_contents_and_report_the_mtime() {
        let home = TempHome::new("atomic");
        let path = home.0.join("nested/dir/a.excalidraw");
        let mtime = write_atomic(&path, VALID).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), VALID);
        assert_eq!(modified(&path).unwrap(), Some(mtime));
        write_atomic(&path, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1,
            "no temporary file left"
        );
        assert_eq!(modified(&home.0.join("absent")).unwrap(), None);
        assert!(matches!(load(&home.0.join("absent")), Loaded::Missing));
    }
}
