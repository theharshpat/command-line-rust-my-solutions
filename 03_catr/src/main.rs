use anyhow::Result;
use clap::Parser;
use std::fs;

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
        let mut non_blank_line_count_so_far = 0;
        for (idx, line) in fs::read_to_string(file)?.lines().enumerate() {
            if &args.numbers == &true {
                println!("{:6}\t{}", idx + 1, line);
                continue;
            }
            if &args.number_nonblank == &true {
                if line.trim().is_empty() {
                    println!("{}", line);
                } else {
                    non_blank_line_count_so_far += 1;
                    println!("{:6}\t{}", non_blank_line_count_so_far, line);
                }
                continue;
            }
            println!("{}", line);
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
