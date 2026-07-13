use anyhow::{Result, anyhow, bail};
use clap::{ArgAction, Parser};
use std::cmp::Ordering::*;
use std::fs::File;
use std::io::{self, BufRead, BufReader};

#[derive(Debug, Parser)]
#[command(version)]
/// Rust version of `comm`
struct Args {
    /// Input file 1
    file1: String,

    /// Input file 2
    file2: String,

    /// Suppress printing of column 1
    #[arg(short('1'), action(ArgAction::SetFalse))]
    show_col1: bool,

    /// Suppress printing of column 2
    #[arg(short('2'), action(ArgAction::SetFalse))]
    show_col2: bool,

    /// Suppress printing of column 3
    #[arg(short('3'), action(ArgAction::SetFalse))]
    show_col3: bool,

    /// Case-insensitive comparison of lines
    #[arg(short)]
    insensitive: bool,

    /// Output delimiter
    #[arg(short, long = "output-delimiter", default_value = "\t")]
    delimiter: String,
}

fn open(filename: &str) -> Result<Box<dyn BufRead>> {
    match filename {
        "-" => Ok(Box::new(BufReader::new(io::stdin()))),
        _ => Ok(Box::new(BufReader::new(
            File::open(filename).map_err(|e| anyhow!("{filename}: {e}"))?,
        ))),
    }
}

fn run(args: Args) -> Result<()> {
    let file1 = &args.file1;
    let file2 = &args.file2;
    let show_col1 = args.show_col1;
    let show_col2 = args.show_col2;
    let show_col3 = args.show_col3;
    let insensitive = args.insensitive;
    let delimiter = &args.delimiter;
    if file1 == "-" && file2 == "-" {
        bail!(r#"Both input files cannot be STDIN ("-")"#);
    }

    let fh1 = open(file1)?;
    let fh2 = open(file2)?;

    let mut lines1 = fh1.lines().map_while(Result::ok).map(|line| {
        if insensitive { line.to_lowercase() } else { line }
    });
    let mut lines2 = fh2.lines().map_while(Result::ok).map(|line| {
        if insensitive { line.to_lowercase() } else { line }
    });

    let mut line1 = lines1.next();
    let mut line2 = lines2.next();

    loop {
        if line1.is_none() && line2.is_none() {
            break;
        }
        match (&line1, &line2) {
            (Some(v1), Some(v2)) => match v1.cmp(v2) {
                Equal => {
                    if show_col3 {
                        if show_col1 {
                            print!("{delimiter}");
                        }
                        if show_col2 {
                            print!("{delimiter}");
                        }
                        println!("{v1}");
                    }
                    line1 = lines1.next();
                    line2 = lines2.next();
                }
                Less => {
                    if show_col1 {
                        println!("{v1}");
                    }
                    line1 = lines1.next();
                }
                Greater => {
                    if show_col2 {
                        if show_col1 {
                            print!("{delimiter}");
                        }
                        println!("{v2}");
                    }
                    line2 = lines2.next();
                }
            },
            (Some(v1), None) => {
                if show_col1 {
                    println!("{v1}");
                }
                line1 = lines1.next();
            }
            (None, Some(v2)) => {
                if show_col2 {
                    if show_col1 {
                        print!("{delimiter}");
                    }
                    println!("{v2}");
                }
                line2 = lines2.next();
            }
            _ => (),
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
