use anyhow::Result;
use clap::{Parser, ValueEnum};
use regex::Regex;

#[derive(Debug, Parser)]
#[command(version, about = "Rust version of `find`")]
struct Args {
    /// Search paths
    #[arg(value_name = "PATH", default_value = ".", num_args = 0..)]
    paths: Vec<String>,

    /// Name
    #[arg(short = 'n', long = "name", value_name = "NAME", num_args = 0..)]
    names: Vec<Regex>,

    /// Entry type
    #[arg(short = 't', long = "type", value_name = "TYPE", num_args = 0..)]
    entry_types: Vec<EntryType>,
}

#[derive(Debug, Clone, ValueEnum)]
enum EntryType {
    #[value(name = "f")]
    File,
    #[value(name = "d")]
    Dir,
    #[value(name = "l")]
    Link,
}

fn run(args: Args) -> Result<()> {
    println!("Args {:#?}", args);
    Ok(())
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
