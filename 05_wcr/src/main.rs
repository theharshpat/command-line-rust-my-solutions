use anyhow::Result;
use clap::Parser;
use std::fs::File;
use std::io::{self, BufRead, BufReader};

#[derive(Debug, Parser)]
#[command(version)]
/// Rust version of `wc`
struct Args {
    /// Input file(s)
    #[arg(value_name = "FILE", default_value = "-")]
    files: Vec<String>,

    /// Show line count
    #[arg(short, long)]
    lines: bool,

    /// Show word count
    #[arg(short, long)]
    words: bool,

    /// Show byte count
    #[arg(short = 'c', long)]
    bytes: bool,

    /// Show character count
    #[arg(short = 'm', long, conflicts_with = "bytes")]
    chars: bool,
}

#[derive(Debug, PartialEq, Default)]
struct FileInfo {
    num_lines: usize,
    num_words: usize,
    num_bytes: usize,
    num_chars: usize,
}

impl std::ops::AddAssign for FileInfo {
    fn add_assign(&mut self, other: Self) {
        self.num_lines += other.num_lines;
        self.num_words += other.num_words;
        self.num_bytes += other.num_bytes;
        self.num_chars += other.num_chars;
    }
}

fn open(filename: &str) -> Result<Box<dyn BufRead>> {
    match filename {
        "-" => Ok(Box::new(BufReader::new(io::stdin()))),
        _ => Ok(Box::new(BufReader::new(File::open(filename)?))),
    }
}

fn run(mut args: Args) -> Result<()> {
    let mut total = FileInfo::default();

    if [args.lines, args.words, args.bytes, args.chars]
        .into_iter()
        .all(|b| !b)
    {
        args.lines = true;
        args.words = true;
        args.bytes = true;
    }

    for filename in &args.files {
        match open(filename) {
            Err(err) => eprintln!("{filename}: {err}"),
            Ok(reader) => match count(reader) {
                Ok(info) => {
                    print_info(&info, &args, filename);
                    total += info;
                }
                Err(err) => eprintln!("{filename}: {err}"),
            },
        }
    }
    if args.files.len() > 1 {
        print_info(&total, &args, "total");
    }
    Ok(())
}

fn print_info(info: &FileInfo, args: &Args, filename: &str) {
    let mut output = String::new();
    if args.lines {
        output.push_str(&format!("{:>8}", info.num_lines));
    }
    if args.words {
        output.push_str(&format!("{:>8}", info.num_words));
    }
    if args.bytes {
        output.push_str(&format!("{:>8}", info.num_bytes));
    } else if args.chars {
        output.push_str(&format!("{:>8}", info.num_chars));
    }
    if filename != "-" {
        output.push_str(&format!(" {filename}"));
    }
    println!("{output}");
}

fn count(mut reader: Box<dyn BufRead>) -> Result<FileInfo> {
    let mut info = FileInfo::default();

    let mut line = String::new();
    loop {
        let line_bytes = reader.read_line(&mut line)?;
        if line_bytes == 0 {
            break;
        }
        info.num_lines += 1;
        info.num_words += line.split_whitespace().count();
        info.num_bytes += line_bytes;
        info.num_chars += line.chars().count();
        line.clear();
    }
    Ok(info)
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{FileInfo, count};
    use std::io::Cursor;

    #[test]
    fn test_count() {
        let text = "I don't want the world.\nI just want your half.\r\n";
        let info = count(Box::new(Cursor::new(text)));
        assert!(info.is_ok());
        let expected = FileInfo {
            num_lines: 2,
            num_words: 10,
            num_chars: 48,
            num_bytes: 48,
        };
        assert_eq!(info.unwrap(), expected);
    }
}
