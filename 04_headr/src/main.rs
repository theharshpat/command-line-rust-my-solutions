use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read},
};

use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    #[arg(value_name = "FILE", default_value = "-")]
    files: Vec<String>,

    #[arg(short = 'n', long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
    lines: u64,

    #[arg(short = 'c', long, conflicts_with = "lines", value_parser = clap::value_parser!(u64).range(1..))]
    bytes: Option<u64>,
}

fn open(filename: &str) -> Result<Box<dyn BufRead>> {
    match filename {
        "-" => Ok(Box::new(BufReader::new(io::stdin()))),
        _ => Ok(Box::new(BufReader::new(File::open(filename)?))),
    }
}

fn run(args: Args) -> Result<()> {
    let to_show_file_name_headers = args.files.len() > 1;
    let num_files = args.files.len();
    for (file_index, filename) in args.files.iter().enumerate() {
        let reader_result = open(filename);
        match reader_result {
            Err(err) => eprintln!("{}: {}", filename, err),
            Ok(mut reader) => {
                if to_show_file_name_headers {
                    println!("==> {} <==", filename);
                }
                if args.bytes.is_some() {
                    let bytes = args.bytes.unwrap();
                    let mut buf = Vec::new();
                    let mut reader = reader.take(bytes);
                    reader.read_to_end(&mut buf)?;
                    print!("{}", String::from_utf8_lossy(&buf));
                } else {
                    for _ in 0..args.lines {
                        let mut buf = String::new();
                        let read_bytes = reader.read_line(&mut buf).unwrap();
                        if read_bytes == 0 {
                            break;
                        }
                        print!("{}", buf);
                    }
                }
                if to_show_file_name_headers && file_index < num_files - 1 {
                    println!();
                }
            }
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
