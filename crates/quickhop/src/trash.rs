//! Moving files to the home trash as the FreeDesktop.org Trash specification describes, and
//! the URI escaping it shares with `file://` URIs.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The home trash: `$XDG_DATA_HOME/Trash`, else `$HOME/.local/share/Trash`. `None` when
/// neither variable holds an absolute path.
pub(crate) fn home_trash(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let absolute = |name: &str| var(name).map(PathBuf::from).filter(|p| p.is_absolute());
    absolute("XDG_DATA_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".local").join("share")))
        .map(|data| data.join("Trash"))
}

/// `path` escaped as in RFC 2396: bytes other than unreserved characters and `/` become
/// `%XX`.
pub(crate) fn escape_path(path: &Path) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut escaped = String::new();
    for &byte in path.as_os_str().as_encoded_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            escaped.push(char::from(byte));
        } else {
            escaped.push('%');
            escaped.push(char::from(HEX[usize::from(byte >> 4)]));
            escaped.push(char::from(HEX[usize::from(byte & 0xF)]));
        }
    }
    escaped
}

/// The `file://` URI of the absolute `path`.
pub(crate) fn file_uri(path: &Path) -> String {
    format!("file://{}", escape_path(path))
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// Moves the absolute `path` into `trash` (its `files/` folder, with an `info/*.trashinfo`
/// record holding the original path and `deleted`, a local `YYYY-MM-DDThh:mm:ss` time) and
/// returns where it went. A name already in the trash gets `.2`, `.3`, … appended. Fails
/// without leaving a record when `path` is missing or on another filesystem than `trash`.
pub(crate) fn move_to(trash: &Path, path: &Path, deleted: &str) -> io::Result<PathBuf> {
    std::fs::symlink_metadata(path)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let (files, info) = (trash.join("files"), trash.join("info"));
    create_private_dir(&files)?;
    create_private_dir(&info)?;
    for n in 1_u32.. {
        let mut candidate = name.to_os_string();
        if n > 1 {
            candidate.push(format!(".{n}"));
        }
        let target = files.join(&candidate);
        if std::fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        let mut record_name = candidate.clone();
        record_name.push(".trashinfo");
        let record = info.join(record_name);
        // Creating the record first claims the name, as the specification requires.
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&record)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let written = write!(
            file,
            "[Trash Info]\nPath={}\nDeletionDate={deleted}\n",
            escape_path(path)
        )
        .and_then(|()| std::fs::rename(path, &target));
        if let Err(error) = written {
            let _ = std::fs::remove_file(&record);
            return Err(error);
        }
        return Ok(target);
    }
    Err(io::Error::other("no free name in the trash"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_like_file_uris() {
        assert_eq!(
            file_uri(Path::new("/tmp/a b/ü%#.txt")),
            "file:///tmp/a%20b/%C3%BC%25%23.txt"
        );
        assert_eq!(escape_path(Path::new("/x/A-z_0.9~")), "/x/A-z_0.9~");
    }

    #[test]
    fn home_trash_prefers_xdg_data_home_and_ignores_relative_paths() {
        let vars = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        assert_eq!(
            home_trash(vars(&[("XDG_DATA_HOME", "/d"), ("HOME", "/h")])),
            Some(PathBuf::from("/d/Trash"))
        );
        assert_eq!(
            home_trash(vars(&[("XDG_DATA_HOME", "rel"), ("HOME", "/h")])),
            Some(PathBuf::from("/h/.local/share/Trash"))
        );
        assert_eq!(home_trash(vars(&[("HOME", "rel")])), None);
    }

    #[test]
    fn moves_files_and_folders_with_records_and_unique_names() {
        let dir = tempfile::tempdir().unwrap();
        let trash = dir.path().join("Trash");
        let first = dir.path().join("a b.txt");
        std::fs::write(&first, "1").unwrap();
        let moved = move_to(&trash, &first, "2026-05-01T10:00:00").unwrap();
        assert_eq!(moved, trash.join("files/a b.txt"));
        assert!(!first.exists());
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "1");
        let record = std::fs::read_to_string(trash.join("info/a b.txt.trashinfo")).unwrap();
        assert_eq!(
            record,
            format!(
                "[Trash Info]\nPath={}\nDeletionDate=2026-05-01T10:00:00\n",
                escape_path(&first)
            )
        );
        assert!(record.contains("a%20b.txt"));

        // The same name again, now a folder with contents.
        std::fs::create_dir_all(first.join("inner")).unwrap();
        let again = move_to(&trash, &first, "2026-05-01T10:00:01").unwrap();
        assert_eq!(again, trash.join("files/a b.txt.2"));
        assert!(again.join("inner").is_dir());
        assert!(trash.join("info/a b.txt.2.trashinfo").is_file());
    }

    #[test]
    fn a_missing_file_leaves_no_record() {
        let dir = tempfile::tempdir().unwrap();
        let trash = dir.path().join("Trash");
        let error = move_to(&trash, &dir.path().join("gone.txt"), "x").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
