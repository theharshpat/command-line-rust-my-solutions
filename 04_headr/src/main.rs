use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read, Write},
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

fn open(filename: &str) -> Option<Box<dyn BufRead>> {
    match filename {
        "-" => Some(Box::new(BufReader::new(io::stdin()))),
        _ => match File::open(filename) {
            Ok(file) => Some(Box::new(BufReader::new(file))),
            Err(err) => {
                eprintln!("{}: {}", filename, err);
                None
            }
        },
    }
}

fn head(mut reader: Box<dyn BufRead>, args: &Args) -> Result<()> {
    if let Some(bytes) = args.bytes {
        let mut buf = Vec::new();
        let mut reader = reader.take(bytes);
        reader.read_to_end(&mut buf)?;
        io::stdout().write_all(&buf)?;
    } else {
        let mut buf = String::new();
        for _ in 0..args.lines {
            let read_bytes = reader.read_line(&mut buf)?;
            if read_bytes == 0 {
                break;
            }
            io::stdout().write_all(buf.as_bytes())?;
            buf.clear();
        }
    }
    Ok(())
}

fn run(args: Args) -> Result<()> {
    let num_files = args.files.len();
    for (file_index, filename) in args.files.iter().enumerate() {
        if let Some(reader) = open(filename) {
            if num_files > 1 {
                println!("{}==> {} <==", if file_index > 0 { "\n" } else { "" }, filename);
            }
            head(reader, &args)?;
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

#[cfg(test)]
mod tests {
    use super::open;
    use std::io::Read;

    #[test]
    fn open_valid_file_returns_some() {
        let reader = open("./tests/inputs/one.txt");
        assert!(reader.is_some());
    }

    #[test]
    fn open_missing_file_returns_none() {
        let reader = open("./does/not/exist.txt");
        assert!(reader.is_none());
    }

    #[test]
    fn open_valid_file_is_readable() {
        let mut reader = open("./tests/inputs/twelve.txt").expect("file should open");
        let mut content = String::new();
        reader.read_to_string(&mut content).unwrap();
        assert!(content.starts_with("one\n"));
    }

    #[test]
    fn open_dash_yields_stdin_handle() {
        let reader = open("-");
        assert!(reader.is_some());
    }
}
