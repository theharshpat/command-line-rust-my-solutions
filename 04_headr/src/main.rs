use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    #[arg(value_name = "FILE", default_value = "-")]
    files: Vec<String>,

    #[arg(short = 'n', long, default_value_t = 10)]
    lines: usize,

    #[arg(short = 'c', long, conflicts_with = "lines")]
    bytes: Option<u64>,
}

fn run(args: Args) -> Result<()> {
    println!("Args: {:#?}", args);
    Ok(())
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
