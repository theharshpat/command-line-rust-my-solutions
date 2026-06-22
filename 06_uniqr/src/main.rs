use anyhow::Result;
use clap::Parser;
use std::fs::File;
use std::io::{self, BufRead, BufReader};

#[derive(Debug, Parser)]
#[command(version)]
/// Rust version of `uniq`
struct Args {
    /// Input file
    #[arg(value_name = "IN_FILE", default_value = "-")]
    in_file: String,

    /// Output file
    #[arg(value_name = "OUT_FILE")]
    out_file: Option<String>,

    /// Show counts
    #[arg(short, long)]
    count: bool,
}

fn run(args: Args) -> Result<()> {
    let mut reader = open(&args.in_file)?;
    let mut buffer = String::new();

    let mut prev = None;
    let mut same_count = 0;
    while reader.read_line(&mut buffer)? > 0 {
        if prev.clone().is_none() {
            prev = Some(buffer.clone());
            same_count = 1;
        } else if buffer == prev.clone().unwrap() {
            same_count += 1;
        } else {
            if args.count {
                print!("{:>4} {}", same_count, prev.clone().unwrap());
            } else {
                print!("{}\n", prev.clone().unwrap());
            }
            prev = Some(buffer.clone());
            same_count = 1;
        }
        buffer.clear();
    }

    if args.count {
        print!("{:>4} {}\n", same_count, prev.clone().unwrap());
    } else {
        print!("{}", prev.clone().unwrap());
    }
    buffer.clear();
    Ok(())
}

fn open(filename: &str) -> Result<Box<dyn BufRead>> {
    match filename {
        "-" => Ok(Box::new(BufReader::new(io::stdin()))),
        _ => Ok(Box::new(BufReader::new(File::open(filename)?))),
    }
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
