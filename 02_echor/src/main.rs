use clap::Parser;

#[derive(Parser)]
#[command(name = "echor", version = "0.1.0", about = "Rust version of `echo`")]
struct Args {
    #[arg(required = true)]
    text: Vec<String>,

    #[arg(short = 'n')]
    omit_newline: bool,
}

fn main() {
    let args = Args::parse();
    let ending = if args.omit_newline { "" } else { "\n" };
    print!("{}{}", args.text.join(" "), ending);
}
