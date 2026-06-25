use anyhow::{Result, anyhow};
use clap::Parser;
use regex::{Regex, RegexBuilder};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::mem;
use walkdir::WalkDir;

#[derive(Debug, Parser)]
#[command(version, about = "Rust version of `grep`")]
struct Args {
    /// Search pattern
    pattern: String,

    /// Input file(s)
    #[arg(default_value = "-", value_name = "FILE")]
    files: Vec<String>,

    /// Case-insensitive
    #[arg(short, long)]
    insensitive: bool,

    /// Recursive search
    #[arg(short, long)]
    recursive: bool,

    /// Count occurrences
    #[arg(short, long)]
    count: bool,

    /// Invert match
    #[arg(short = 'v', long = "invert-match")]
    invert: bool,
}

fn find_files(files: &[String], recursive: bool) -> Vec<Result<String>> {
    let mut result = Vec::new();

    for file in files {
        if file == "-" {
            result.push(Ok(file.to_string()));
            continue;
        }

        let meta = match fs::metadata(file) {
            Ok(m) => m,
            Err(e) => {
                result.push(Err(anyhow!("{}: {}", file, e)));
                continue;
            }
        };

        if meta.is_file() {
            result.push(Ok(file.to_string()));
            continue;
        }

        if !recursive {
            result.push(Err(anyhow!("{} is a directory", file)));
            continue;
        }

        for entry in WalkDir::new(file) {
            match entry {
                Ok(entry) if entry.file_type().is_file() => {
                    result.push(Ok(entry.path().display().to_string()));
                }
                Ok(_) => {}
                Err(e) => result.push(Err(anyhow!("{}", e))),
            }
        }
    }

    result
}

fn open(filename: &str) -> Result<Box<dyn BufRead>> {
    match filename {
        "-" => Ok(Box::new(BufReader::new(io::stdin()))),
        _ => Ok(Box::new(BufReader::new(File::open(filename)?))),
    }
}

fn find_lines<T: BufRead>(mut reader: T, pattern: &Regex, invert: bool) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        if !line.ends_with('\n') {
            line.push('\n');
        }
        if pattern.is_match(&line) ^ invert {
            result.push(mem::take(&mut line));
        }
    }
    Ok(result)
}

fn run(args: Args) -> Result<()> {
    let pattern = RegexBuilder::new(&args.pattern)
        .case_insensitive(args.insensitive)
        .build()
        .map_err(|_| anyhow!(r#"Invalid pattern "{}""#, args.pattern))?;

    let files = find_files(&args.files, args.recursive);
    let to_print_file_prefix = args.recursive || args.files.len() > 1;
    let print = |fname: &str, val: &str| {
        if to_print_file_prefix {
            print!("{fname}:{val}");
        } else {
            print!("{val}");
        }
    };

    for file in files {
        match file {
            Err(e) => eprintln!("{e}"),
            Ok(file) => {
                let reader = match open(&file) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("{file}: {e}");
                        continue;
                    }
                };
                let lines = find_lines(reader, &pattern, args.invert);
                let lines = lines.unwrap();

                let display_name = if file == "-" {
                    "(standard input)".to_string()
                } else {
                    file
                };

                if args.count {
                    print(&display_name, &format!("{}\n", lines.len()));
                } else if !lines.is_empty() {
                    for l in lines {
                        print(&display_name, &l);
                    }
                }
            }
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{find_files, find_lines};
    use pretty_assertions::assert_eq;
    use rand::{RngExt, distr::Alphanumeric};
    use regex::{Regex, RegexBuilder};
    use std::io::Cursor;

    #[test]
    fn test_find_files() {
        // Verify that the function finds a file known to exist
        let files = find_files(&["./tests/inputs/fox.txt".to_string()], false);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].as_ref().unwrap(), "./tests/inputs/fox.txt");

        // The function should reject a directory without the recursive option
        let files = find_files(&["./tests/inputs".to_string()], false);
        assert_eq!(files.len(), 1);
        if let Err(e) = &files[0] {
            assert_eq!(e.to_string(), "./tests/inputs is a directory");
        }

        // Verify the function recurses to find four files in the directory
        let res = find_files(&["./tests/inputs".to_string()], true);
        let mut files: Vec<String> = res
            .iter()
            .map(|r| r.as_ref().unwrap().replace("\\", "/"))
            .collect();
        files.sort();
        assert_eq!(files.len(), 4);
        assert_eq!(
            files,
            vec![
                "./tests/inputs/bustle.txt",
                "./tests/inputs/empty.txt",
                "./tests/inputs/fox.txt",
                "./tests/inputs/nobody.txt",
            ]
        );

        // Generate a random string to represent a nonexistent file
        let bad: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();

        // Verify that the function returns the bad file as an error
        let files = find_files(&[bad], false);
        assert_eq!(files.len(), 1);
        assert!(files[0].is_err());
    }

    #[test]
    fn test_find_lines() {
        let text = b"Lorem\nIpsum\r\nDOLOR";

        // The pattern _or_ should match the one line, "Lorem"
        let re1 = Regex::new("or").unwrap();
        let matches = find_lines(Cursor::new(&text), &re1, false);
        assert!(matches.is_ok());
        assert_eq!(matches.unwrap().len(), 1);

        // When inverted, the function should match the other two lines
        let matches = find_lines(Cursor::new(&text), &re1, true);
        assert!(matches.is_ok());
        assert_eq!(matches.unwrap().len(), 2);

        // This regex will be case-insensitive
        let re2 = RegexBuilder::new("or")
            .case_insensitive(true)
            .build()
            .unwrap();

        // The two lines "Lorem" and "DOLOR" should match
        let matches = find_lines(Cursor::new(&text), &re2, false);
        assert!(matches.is_ok());
        assert_eq!(matches.unwrap().len(), 2);

        // When inverted, the one remaining line should match
        let matches = find_lines(Cursor::new(&text), &re2, true);
        assert!(matches.is_ok());
        assert_eq!(matches.unwrap().len(), 1);
    }
}
