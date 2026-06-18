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

fn run(mut args: Args) -> Result<()> {
    let mut total = FileInfo {
        num_lines: 0,
        num_words: 0,
        num_bytes: 0,
        num_chars: 0,
    };

    if [args.lines, args.words, args.bytes, args.chars]
        .into_iter()
        .all(|b| b == false)
    {
        args.lines = true;
        args.words = true;
        args.bytes = true;
    }

    for filename in &args.files {
        let reader = open(&filename)?;
        let info = count(reader);
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

fn count(mut reader: Box<dyn BufRead>) -> FileInfo {
    let mut info = FileInfo {
        num_lines: 0,
        num_words: 0,
        num_bytes: 0,
        num_chars: 0,
    };

    let mut line = String::new();
    loop {
        let line_bytes = reader.read_line(&mut line).unwrap();
        if line_bytes == 0 {
            break;
        }
        info.num_lines += 1;
        info.num_words += line.split_whitespace().count();
        info.num_bytes += line_bytes;
        info.num_chars += line.chars().count();
        line.clear();
    }
    info
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
