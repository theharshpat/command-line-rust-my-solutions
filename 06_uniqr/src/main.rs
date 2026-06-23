use anyhow::{Result, anyhow};
use clap::Parser;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};

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
    let mut file = open(&args.in_file).map_err(|e| anyhow!("{}: {e}", args.in_file))?;

    let mut writer =
        write(&args.out_file).map_err(|e| anyhow!("{}: {e}", args.out_file.unwrap()))?;

    let mut line = String::new();

    let mut prev = None;
    let mut same_count = 0;

    loop {
        let bytes_read = file.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }

        if prev.clone().is_none() {
            prev = Some(line.clone());
            same_count = 1;
        } else if line.trim_end() == prev.clone().unwrap().trim_end() {
            same_count += 1;
        } else {
            if args.count {
                write!(writer, "{:>4} {}", same_count, prev.clone().unwrap())?;
            } else {
                write!(writer, "{}", prev.clone().unwrap())?;
            }
            prev = Some(line.clone());
            same_count = 1;
        }

        line.clear();
    }

    if prev.clone().is_some() {
        if args.count {
            write!(writer, "{:>4} {}", same_count, prev.clone().unwrap())?;
        } else {
            write!(writer, "{}", prev.clone().unwrap())?;
        }
    }

    Ok(())
}

fn open(filename: &str) -> Result<Box<dyn BufRead>> {
    match filename {
        "-" => Ok(Box::new(BufReader::new(io::stdin()))),
        _ => Ok(Box::new(BufReader::new(File::open(filename)?))),
    }
}

fn write(filename: &Option<String>) -> Result<Box<dyn Write>> {
    match filename {
        None => Ok(Box::new(io::stdout())),
        Some(filename) => Ok(Box::new(File::create(filename)?)),
    }
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
