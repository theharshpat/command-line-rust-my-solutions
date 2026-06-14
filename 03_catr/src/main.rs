use anyhow::Result;
use clap::Parser;
use std::{
    fs::File,
    io::{self, BufRead, BufReader},
};

#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    #[arg(value_name = "FILE", default_value = "-")]
    files: Vec<String>,

    #[arg(short, long, conflicts_with = "number_nonblank")]
    numbers: bool,

    #[arg(long, short = 'b')]
    number_nonblank: bool,
}

fn run(args: Args) -> Result<()> {
    for file in &args.files {
        let reader: Box<dyn BufRead> = if file == "-" {
            Box::new(BufReader::new(io::stdin()))
        } else {
            Box::new(BufReader::new(File::open(file)?))
        };

        let mut non_blank_line_count_so_far = 0;

        for (idx, line) in reader.lines().enumerate() {
            let line = line?;

            if args.numbers {
                println!("{:>6}\t{}", idx + 1, line);
                continue;
            }

            if args.number_nonblank {
                if line.is_empty() {
                    println!();
                } else {
                    non_blank_line_count_so_far += 1;

                    println!("{:>6}\t{}", non_blank_line_count_so_far, line);
                }

                continue;
            }

            println!("{line}");
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
