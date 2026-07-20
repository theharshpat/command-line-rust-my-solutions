use anyhow::Result;
use chrono::{DateTime, Local};
use clap::Parser;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use users::{get_group_by_gid, get_user_by_uid};

mod owner;
pub use owner::Owner;

#[derive(Debug, Parser)]
#[command(version, about = "Rust version of `ls`")]
pub struct Args {
    /// Files or directories to list
    #[arg(default_value = ".", value_name = "PATH")]
    paths: Vec<String>,

    /// Use a long listing format
    #[arg(short, long)]
    long: bool,

    /// Show hidden directory entries
    #[arg(short, long)]
    all: bool,
}

fn run(args: Args) -> Result<()> {
    let files = find_files(&args.paths, args.all)?;

    if !args.long {
        for path in files {
            println!("{}", path.display());
        }
    }
    Ok(())
}

fn find_files(paths: &[String], show_hidden: bool) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for path in paths {
        let path = PathBuf::from(path);
        let metadata = match path.metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
                eprintln!("{}: {err}", path.display());
                continue;
            }
        };

        if metadata.is_file() {
            files.push(path);
        } else if metadata.is_dir() {
            match fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if show_hidden || !entry.file_name().to_string_lossy().starts_with('.') {
                            files.push(entry.path());
                        }
                    }
                }
                Err(err) => eprintln!("{}: {err}", path.display()),
            }
        }
    }

    Ok(files)
}

fn mk_triple(mode: u32, owner: Owner) -> String {
    let [read_mask, write_mask, execute_mask] = owner.masks();

    let read = if mode & read_mask != 0 { 'r' } else { '-' };
    let write = if mode & write_mask != 0 { 'w' } else { '-' };
    let execute = if mode & execute_mask != 0 { 'x' } else { '-' };

    format!("{read}{write}{execute}")
}

fn format_mode(mode: u32) -> String {
    format!(
        "{}{}{}",
        mk_triple(mode, Owner::User),
        mk_triple(mode, Owner::Group),
        mk_triple(mode, Owner::Other),
    )
}

fn format_output(paths: &[PathBuf]) -> Result<String> {
    let mut output = String::new();

    for path in paths {
        let metadata = path.metadata()?;
        let file_type = if metadata.is_dir() { 'd' } else { '-' };
        let user = get_user_by_uid(metadata.uid())
            .map(|user| user.name().to_string_lossy().into_owned())
            .unwrap_or_else(|| metadata.uid().to_string());
        let group = get_group_by_gid(metadata.gid())
            .map(|group| group.name().to_string_lossy().into_owned())
            .unwrap_or_else(|| metadata.gid().to_string());
        let modified: DateTime<Local> = metadata.modified()?.into();
        let modified = modified.format("%b %d %y %H:%M");

        output.push_str(&format!(
            "{file_type}{} {} {} {} {} {} {}\n",
            format_mode(metadata.mode()),
            metadata.nlink(),
            user,
            group,
            metadata.len(),
            modified,
            path.display(),
        ));
    }

    Ok(output)
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod test {
    use super::{find_files, format_mode, format_output, mk_triple, Owner};
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    #[test]
    fn test_find_files() {
        // Find all non-hidden entries in a directory
        let res = find_files(&["tests/inputs".to_string()], false);
        assert!(res.is_ok());
        let mut filenames: Vec<_> = res
            .unwrap()
            .iter()
            .map(|entry| entry.display().to_string())
            .collect();
        filenames.sort();
        assert_eq!(
            filenames,
            [
                "tests/inputs/bustle.txt",
                "tests/inputs/dir",
                "tests/inputs/empty.txt",
                "tests/inputs/fox.txt",
            ]
        );

        // Any existing file should be found even if hidden
        let res = find_files(&["tests/inputs/.hidden".to_string()], false);
        assert!(res.is_ok());
        let filenames: Vec<_> = res
            .unwrap()
            .iter()
            .map(|entry| entry.display().to_string())
            .collect();
        assert_eq!(filenames, ["tests/inputs/.hidden"]);

        // Test multiple path arguments
        let res = find_files(
            &[
                "tests/inputs/bustle.txt".to_string(),
                "tests/inputs/dir".to_string(),
            ],
            false,
        );
        assert!(res.is_ok());
        let mut filenames: Vec<_> = res
            .unwrap()
            .iter()
            .map(|entry| entry.display().to_string())
            .collect();
        filenames.sort();
        assert_eq!(
            filenames,
            ["tests/inputs/bustle.txt", "tests/inputs/dir/spiders.txt"]
        );
    }

    #[test]
    fn test_find_files_hidden() {
        // Find all entries in a directory including hidden
        let res = find_files(&["tests/inputs".to_string()], true);
        assert!(res.is_ok());
        let mut filenames: Vec<_> = res
            .unwrap()
            .iter()
            .map(|entry| entry.display().to_string())
            .collect();
        filenames.sort();
        assert_eq!(
            filenames,
            [
                "tests/inputs/.hidden",
                "tests/inputs/bustle.txt",
                "tests/inputs/dir",
                "tests/inputs/empty.txt",
                "tests/inputs/fox.txt",
            ]
        );
    }

    fn long_match(
        line: &str,
        expected_name: &str,
        expected_perms: &str,
        expected_size: Option<&str>,
    ) {
        let parts: Vec<_> = line.split_whitespace().collect();
        assert!(!parts.is_empty() && parts.len() <= 10);

        let perms = parts.first().unwrap();
        assert_eq!(perms, &expected_perms);

        if let Some(size) = expected_size {
            let file_size = parts.get(4).unwrap();
            assert_eq!(file_size, &size);
        }

        let display_name = parts.last().unwrap();
        assert_eq!(display_name, &expected_name);
    }

    #[test]
    fn test_format_output_one() {
        let bustle_path = "tests/inputs/bustle.txt";
        let bustle = PathBuf::from(bustle_path);

        let res = format_output(&[bustle]);
        assert!(res.is_ok());

        let out = res.unwrap();
        let lines: Vec<&str> = out.split('\n').filter(|s| !s.is_empty()).collect();
        assert_eq!(lines.len(), 1);

        let line1 = lines.first().unwrap();
        long_match(line1, bustle_path, "-rw-r--r--", Some("193"));
    }

    #[test]
    fn test_format_output_two() {
        let res = format_output(&[
            PathBuf::from("tests/inputs/dir"),
            PathBuf::from("tests/inputs/empty.txt"),
        ]);
        assert!(res.is_ok());

        let out = res.unwrap();
        let mut lines: Vec<&str> = out.split('\n').filter(|s| !s.is_empty()).collect();
        lines.sort();
        assert_eq!(lines.len(), 2);

        let empty_line = lines.remove(0);
        long_match(
            empty_line,
            "tests/inputs/empty.txt",
            "-rw-r--r--",
            Some("0"),
        );

        let dir_line = lines.remove(0);
        long_match(dir_line, "tests/inputs/dir", "drwxr-xr-x", None);
    }

    #[test]
    fn test_mk_triple() {
        assert_eq!(mk_triple(0o751, Owner::User), "rwx");
        assert_eq!(mk_triple(0o751, Owner::Group), "r-x");
        assert_eq!(mk_triple(0o751, Owner::Other), "--x");
        assert_eq!(mk_triple(0o600, Owner::Other), "---");
    }

    #[test]
    fn test_format_mode() {
        assert_eq!(format_mode(0o755), "rwxr-xr-x");
        assert_eq!(format_mode(0o421), "r---w---x");
    }
}
