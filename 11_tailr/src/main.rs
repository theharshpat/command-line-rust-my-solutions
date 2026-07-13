use crate::TakeValue::*;
use anyhow::{Result, anyhow};
use clap::Parser;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::ops::Neg;

#[derive(Debug, Parser)]
#[command(version, about = "Rust version of `tail`")]
struct Args {
    /// Input file(s)
    #[arg(required = true, value_name = "FILE")]
    files: Vec<String>,

    /// Number of lines
    #[arg(short = 'n', long = "lines", default_value = "10")]
    lines: String,

    /// Number of bytes
    #[arg(
        short = 'c',
        long = "bytes",
        value_name = "BYTES",
        conflicts_with = "lines"
    )]
    bytes: Option<String>,

    /// Suppress headers
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Debug, PartialEq)]
enum TakeValue {
    PlusZero,
    TakeNum(i64),
}

fn parse_num(val: String) -> Result<TakeValue> {
    // +0 is special: means "everything from the start"
    if val == "+0" {
        return Ok(PlusZero);
    }

    // All other inputs must parse as i64
    let n = val.parse::<i64>().map_err(|_| anyhow!("{val}"))?;

    // +N → positive (pass through); -N → negative (pass through);
    // bare N → negative (tail semantics: "last N")
    let signed = if val.starts_with('+') || val.starts_with('-') {
        n
    } else {
        n.neg()
    };
    Ok(TakeNum(signed))
}

fn count_lines_bytes(filename: &str) -> Result<(i64, i64)> {
    let mut file = BufReader::new(File::open(filename)?);
    let mut num_lines = 0;
    let mut num_bytes = 0;
    let mut buf = Vec::new();
    loop {
        let n = file.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        num_lines += 1;
        num_bytes += n as i64;
        buf.clear();
    }
    Ok((num_lines, num_bytes))
}

fn run(args: Args) -> Result<()> {
    let lines = parse_num(args.lines).map_err(|e| anyhow!("illegal line count -- {e}"))?;
    let bytes = args
        .bytes
        .map(parse_num)
        .transpose()
        .map_err(|e| anyhow!("illegal byte count -- {e}"))?;
    println!("lines = {lines:?}");
    println!("bytes = {bytes:?}");

    for filename in args.files {
        match File::open(&filename) {
            Err(err) => eprintln!("{filename}: {err}"),
            Ok(_) => {
                let (total_lines, total_bytes) = count_lines_bytes(&filename)?;
                println!("{filename} has {total_lines} lines, {total_bytes} bytes");
            }
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

#[cfg(test)]
mod tests {
    use super::{TakeValue::*, count_lines_bytes, parse_num};
    use pretty_assertions::assert_eq;

    #[test]
    fn test_count_lines_bytes() {
        let res = count_lines_bytes("tests/inputs/one.txt");
        assert!(res.is_ok());
        let (lines, bytes) = res.unwrap();
        assert_eq!(lines, 1);
        assert_eq!(bytes, 24);

        let res = count_lines_bytes("tests/inputs/twelve.txt");
        assert!(res.is_ok());
        let (lines, bytes) = res.unwrap();
        assert_eq!(lines, 12);
        assert_eq!(bytes, 63);
    }

    #[test]
    fn test_get_start_index() {
        // +0 from an empty file (0 lines/bytes) returns None
        assert_eq!(get_start_index(&PlusZero, 0), None);

        // +0 from a nonempty file returns an index that
        // is one less than the number of lines/bytes
        assert_eq!(get_start_index(&PlusZero, 1), Some(0));

        // Taking 0 lines/bytes returns None
        assert_eq!(get_start_index(&TakeNum(0), 1), None);

        // Taking any lines/bytes from an empty file returns None
        assert_eq!(get_start_index(&TakeNum(1), 0), None);

        // Taking more lines/bytes than is available returns None
        assert_eq!(get_start_index(&TakeNum(2), 1), None);

        // When starting line/byte is less than total lines/bytes,
        // return one less than starting number
        assert_eq!(get_start_index(&TakeNum(1), 10), Some(0));
        assert_eq!(get_start_index(&TakeNum(2), 10), Some(1));
        assert_eq!(get_start_index(&TakeNum(3), 10), Some(2));

        // When starting line/byte is negative and less than total,
        // return total - start
        assert_eq!(get_start_index(&TakeNum(-1), 10), Some(9));
        assert_eq!(get_start_index(&TakeNum(-2), 10), Some(8));
        assert_eq!(get_start_index(&TakeNum(-3), 10), Some(7));

        // When the starting line/byte is negative and more than the total,
        // return 0 to print the whole file
        assert_eq!(get_start_index(&TakeNum(-20), 10), Some(0));
    }

    #[test]
    fn test_parse_num() {
        // All integers should be interpreted as negative numbers
        let res = parse_num("3".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), TakeNum(-3));

        // A leading "+" should result in a positive number
        let res = parse_num("+3".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), TakeNum(3));

        // An explicit "-" value should result in a negative number
        let res = parse_num("-3".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), TakeNum(-3));

        // Zero is zero
        let res = parse_num("0".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), TakeNum(0));

        // Plus zero is special
        let res = parse_num("+0".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), PlusZero);

        // Test boundaries
        let res = parse_num(i64::MAX.to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), TakeNum(i64::MIN + 1));

        let res = parse_num((i64::MIN + 1).to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), TakeNum(i64::MIN + 1));

        let res = parse_num(format!("+{}", i64::MAX));
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), TakeNum(i64::MAX));

        let res = parse_num(i64::MIN.to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), TakeNum(i64::MIN));

        // A floating-point value is invalid
        let res = parse_num("3.14".to_string());
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), "3.14");

        // Any non-integer string is invalid
        let res = parse_num("foo".to_string());
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), "foo");
    }
}
