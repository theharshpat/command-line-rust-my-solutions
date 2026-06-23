use anyhow::Result;
use clap::{Parser, ValueEnum};
use regex::Regex;
use walkdir::{DirEntry, WalkDir};

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

    /// Maximum recursion depth (0 = only the root)
    #[arg(long)]
    max_depth: Option<usize>,

    /// Minimum recursion depth (0 = the root)
    #[arg(long)]
    min_depth: Option<usize>,
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
    let type_filter = |entry: &DirEntry| {
        args.entry_types.is_empty()
            || args.entry_types.iter().any(|entry_type| match entry_type {
                EntryType::Link => entry.file_type().is_symlink(),
                EntryType::Dir => entry.file_type().is_dir(),
                EntryType::File => entry.file_type().is_file(),
            })
    };

    let name_filter = |entry: &DirEntry| {
        args.names.is_empty()
            || args
                .names
                .iter()
                .any(|re| re.is_match(&entry.file_name().to_string_lossy()))
    };

    for path in &args.paths {
        let mut walker = WalkDir::new(path);
        if let Some(d) = args.min_depth {
            walker = walker.min_depth(d);
        }
        if let Some(d) = args.max_depth {
            walker = walker.max_depth(d);
        }
        for entry in walker
            .into_iter()
            .filter_map(|e| match e {
                Err(e) => {
                    eprintln!("{e}");
                    None
                }
                Ok(entry) => Some(entry),
            })
            .filter(type_filter)
            .filter(name_filter)
        {
            println!("{}", entry.path().display());
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
