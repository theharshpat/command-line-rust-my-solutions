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

#[derive(Debug, PartialEq)]
struct FileInfo {
    num_lines: usize,
    num_words: usize,
    num_bytes: usize,
    num_chars: usize,
}

fn open(filename: &str) -> Result<Box<dyn BufRead>> {
    match filename {
        "-" => Ok(Box::new(BufReader::new(io::stdin()))),
        _ => Ok(Box::new(BufReader::new(File::open(filename)?))),
    }
}

fn run(args: Args) -> Result<()> {
    let mut total = FileInfo {
        num_lines: 0,
        num_words: 0,
        num_bytes: 0,
        num_chars: 0,
    };

    for filename in &args.files {
        let reader = open(&filename)?;
        let info = count(reader, &args);
        print_info(&info, &args, &filename);
        total.num_lines += info.num_lines;
        total.num_words += info.num_words;
        total.num_bytes += info.num_bytes;
        total.num_chars += info.num_chars;
    }
    if args.files.len() > 1 {
        print_info(&total, &args, "total");
    }
    Ok(())
}

fn print_info(info: &FileInfo, args: &Args, filename: &str) {
    println!(
        "{}{}{} {}",
        if args.lines {
            format!("{:>8}", info.num_lines)
        } else {
            format!("")
        },
        if args.words {
            format!("{:>8}", info.num_words)
        } else {
            format!("")
        },
        if args.bytes {
            format!("{:>8}", info.num_bytes)
        } else if args.chars {
            format!("{:>8}", info.num_chars)
        } else {
            format!("")
        },
        filename
    )
}

fn count(reader: Box<dyn BufRead>, args: &Args) -> FileInfo {
    let mut info = FileInfo {
        num_lines: 0,
        num_words: 0,
        num_bytes: 0,
        num_chars: 0,
    };

    for line in reader.lines() {
        let line = line.unwrap();
        info.num_lines += 1;
        info.num_words += line.split_whitespace().count();
        if args.chars {
            info.num_chars += line.chars().count();
        } else {
            info.num_bytes += line.len();
        }
    }
    info
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
