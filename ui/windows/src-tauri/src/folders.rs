//! A folder of `.sql` files linked into the sidebar (`docs/decisions/0016-sql-files.md`):
//! the files under it, read once each time the pane shows or is refreshed.

use std::path::Path;

use kemable_core::DriverError;
use serde::Serialize;

/// The most files one folder lists: a script folder holds tens or hundreds;
/// past this it is most likely not one (a home folder, a drive).
pub const MAX_FILES: usize = 5_000;
/// How deep the walk goes below the folder linked.
const MAX_DEPTH: usize = 12;
/// Folders of tools and builds, never of scripts, which can hold many files.
const SKIPPED: [&str; 4] = ["node_modules", "target", "bin", "obj"];

/// What a linked folder holds, folders first, then files, each by name.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlFolder {
    pub entries: Vec<SqlFolderEntry>,
    /// The listing stopped at [`MAX_FILES`].
    pub truncated: bool,
    #[serde(skip)]
    files: usize,
}

/// A file, or a folder that has one somewhere below it.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlFolderEntry {
    /// The path below the folder linked, its parts joined by `/`.
    pub relative: String,
    /// The full path, to open.
    pub path: String,
    pub folder: bool,
    /// How many folders down from the one linked: 0 for what sits in it.
    pub depth: usize,
}

/// Lists the `.sql` files under `root`, leaving out hidden folders (`.git`),
/// those of tools and builds, and folders with no `.sql` file below them.
pub fn list(root: &Path) -> Result<SqlFolder, DriverError> {
    let metadata = std::fs::metadata(root).map_err(|error| io_error(root, error))?;
    if !metadata.is_dir() {
        return Err(DriverError::new(
            kemable_core::ErrorCode::Io,
            format!("{} is not a folder", root.display()),
        ));
    }
    let mut listing = SqlFolder::default();
    walk(root, "", 0, &mut listing);
    Ok(listing)
}

fn io_error(path: &Path, error: std::io::Error) -> DriverError {
    DriverError::new(
        kemable_core::ErrorCode::Io,
        format!("cannot read {}: {error}", path.display()),
    )
}

/// Adds the folders and files of `folder`; true when it added any. A folder
/// that cannot be read is left out.
fn walk(folder: &Path, relative: &str, depth: usize, listing: &mut SqlFolder) -> bool {
    let Ok(read) = std::fs::read_dir(folder) else {
        return false;
    };
    let mut folders = Vec::new();
    let mut files = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if !name.starts_with('.') && !SKIPPED.iter().any(|skipped| skipped.eq_ignore_ascii_case(&name)) {
                folders.push(name);
            }
        } else if Path::new(&name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("sql"))
        {
            files.push(name);
        }
    }
    let by_name = |a: &String, b: &String| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b));
    folders.sort_by(by_name);
    files.sort_by(by_name);
    let join = |name: &str| {
        if relative.is_empty() {
            name.to_owned()
        } else {
            format!("{relative}/{name}")
        }
    };
    let mut added = false;
    if depth < MAX_DEPTH {
        for name in folders {
            if listing.truncated {
                break;
            }
            let path = folder.join(&name);
            let inner = join(&name);
            // The folder's row goes before what it holds, and is taken out again when it holds nothing.
            let at = listing.entries.len();
            listing.entries.push(SqlFolderEntry {
                relative: inner.clone(),
                path: path.to_string_lossy().into_owned(),
                folder: true,
                depth,
            });
            if walk(&path, &inner, depth + 1, listing) {
                added = true;
            } else {
                listing.entries.truncate(at);
            }
        }
    }
    for name in files {
        if listing.files >= MAX_FILES {
            listing.truncated = true;
            break;
        }
        listing.entries.push(SqlFolderEntry {
            relative: join(&name),
            path: folder.join(&name).to_string_lossy().into_owned(),
            folder: false,
            depth,
        });
        listing.files += 1;
        added = true;
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_lists_its_sql_files_below_it_folders_first_and_leaves_out_the_rest() {
        let root = std::env::temp_dir().join(format!("kemable-sql-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for folder in [
            "reports/monthly",
            "empty/inner",
            ".git",
            "node_modules/pkg",
            "Migrations",
        ] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        for file in [
            "b.sql",
            "A.SQL",
            "notes.txt",
            "reports/sales.sql",
            "reports/monthly/june.sql",
            "empty/inner/readme.md",
            ".git/hook.sql",
            "node_modules/pkg/x.sql",
            "Migrations/001_init.sql",
        ] {
            std::fs::write(root.join(file), "select 1;").unwrap();
        }
        let listing = list(&root).unwrap();
        let rows: Vec<(String, bool, usize)> = listing
            .entries
            .iter()
            .map(|entry| (entry.relative.clone(), entry.folder, entry.depth))
            .collect();
        assert_eq!(
            rows,
            [
                ("Migrations".to_owned(), true, 0),
                ("Migrations/001_init.sql".to_owned(), false, 1),
                ("reports".to_owned(), true, 0),
                ("reports/monthly".to_owned(), true, 1),
                ("reports/monthly/june.sql".to_owned(), false, 2),
                ("reports/sales.sql".to_owned(), false, 1),
                ("A.SQL".to_owned(), false, 0),
                ("b.sql".to_owned(), false, 0),
            ]
        );
        assert!(!listing.truncated);
        assert_eq!(
            listing.entries[1].path,
            root.join("Migrations").join("001_init.sql").to_string_lossy()
        );
        assert!(list(&root.join("b.sql")).is_err(), "a file is not a folder");
        assert!(list(&root.join("gone")).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
