use crate::TakeValue::*;
use anyhow::{Result, anyhow};
use clap::Parser;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
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

fn count_lines_bytes(file: &mut impl BufRead) -> Result<(i64, i64)> {
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

fn get_start_index(take_val: &TakeValue, total: i64) -> Option<u64> {
    match take_val {
        TakeValue::PlusZero => {
            if total > 0 {
                Some(0)
            } else {
                None
            }
        }
        TakeValue::TakeNum(num) => {
            if num == &0 || total == 0 || num > &total {
                None
            } else {
                let start = if num < &0 { total + num } else { num - 1 };
                Some(if start < 0 { 0 } else { start as u64 })
            }
        }
    }
}

fn print_lines<T: BufRead>(mut file: T, num_lines: &TakeValue, total_lines: i64) -> Result<()> {
    if let Some(start) = get_start_index(num_lines, total_lines) {
        let mut line_num = 0;
        let mut buf = Vec::new();
        loop {
            let bytes_read = file.read_until(b'\n', &mut buf)?;
            if bytes_read == 0 {
                break;
            }
            if line_num >= start {
                print!("{}", String::from_utf8_lossy(&buf));
            }
            line_num += 1;
            buf.clear();
        }
    }
    Ok(())
}

fn print_bytes(mut file: File, num_bytes: &TakeValue, total_bytes: i64) -> Result<()> {
    if let Some(start) = get_start_index(num_bytes, total_bytes) {
        file.seek(SeekFrom::Start(start))?;
        let stdout = io::stdout();
        let mut out = stdout.lock();
        io::copy(&mut file, &mut out)?;
    }
    Ok(())
}

/// Find the byte offset where the last `n` lines begin.
///
/// Scans backwards from EOF in 8 KB chunks, counting newlines.
/// Returns a byte offset; `io::copy` from there to EOF reproduces `tail -n n`.
///
/// # The trailing-newline edge case
///
/// The last byte in the file is either `\n` (last line is terminated) or
/// something else (last line runs up to EOF with no terminator). This
/// determines `to_skip` — how many `\n` must be counted before the *next*
/// `\n` becomes the boundary.
///
/// ```text
/// Case 1: file ends with `\n` — n=2  →  to_skip = n = 2
///
///   1 │ a\n
///   2 │ b\n   ← last 2 lines start here
///   3 │ c\n
///     └ <EOF>
///
///   Scan back from <EOF>:
///     skip `\n` ending line 3   (skipped = 1)
///     skip `\n` ending line 2   (skipped = 2 = to_skip)
///     hit  `\n` ending line 1   → boundary, start = byte after it = "b"
///   Output: "b\nc\n" ✓
///
///
/// Case 2: file does NOT end with `\n` — n=2  →  to_skip = n - 1 = 1
///
///   1 │ a\n
///   2 │ b\n   ← last 2 lines start here
///   3 │ c
///     └ <EOF>
///
///   Scan back from <EOF>:
///     skip `\n` ending line 2   (skipped = 1 = to_skip)
///     hit  `\n` ending line 1   → boundary, start = byte after it = "b"
///   Output: "b\nc" ✓
///   (Line 3 "c" ends implicitly at EOF — no `\n` to skip for it.)
///
///
/// Case 3: n=1, file does NOT end with `\n`  →  to_skip = 0
///
///   1 │ a\n
///   2 │ b\n
///   3 │ c     ← last 1 line starts here
///     └ <EOF>
///
///   to_skip = 0, so the FIRST `\n` found is the boundary immediately.
///   Output: "c" ✓
///
///
/// Case 4: file has fewer lines than n — n=10, no newlines at all
///
///   1 │ abc
///     └ <EOF>
///
///   to_skip = 9, but the scan exhausts all bytes without finding a `\n`.
///   → return 0, print the whole file.
///   Output: "abc" ✓
///
///
/// Case 5: empty file — n=10
///
///   └ <EOF>
///
///   total = 0 → return 0 immediately.
///   io::copy copies 0 bytes → no output. ✓
/// ```
fn find_last_lines_start(file: &mut File, n: usize) -> Result<u64> {
    let total = file.metadata()?.len();
    if total == 0 {
        return Ok(0);
    }

    // Check if file ends with '\n' by reading the last byte.
    let mut last = [0u8; 1];
    file.seek(SeekFrom::End(-1))?;
    file.read_exact(&mut last)?;
    let ends_with_newline = last[0] == b'\n';

    // If the file ends with '\n', that newline ends the last line; skip it
    // along with the newline ending each preceding "last" line. If the file
    // does not end with '\n', the last line ends implicitly at EOF, so we
    // skip one fewer newline (the last line has no terminator to skip).
    let to_skip = if ends_with_newline {
        n
    } else {
        n.saturating_sub(1)
    };

    const BLOCK: usize = 8192;
    let mut buf = vec![0u8; BLOCK];
    let mut remaining = total;
    let mut skipped: usize = 0;

    while remaining > 0 {
        let read_size = std::cmp::min(BLOCK as u64, remaining) as usize;
        let block_start = remaining - read_size as u64;

        file.seek(SeekFrom::Start(block_start))?;
        file.read_exact(&mut buf[..read_size])?;

        // Scan this block backwards. Once we've skipped the required number
        // of newlines, the next newline we find is the boundary; the start
        // of the last N lines is the byte immediately after it.
        for i in (0..read_size).rev() {
            if buf[i] == b'\n' {
                if skipped < to_skip {
                    skipped += 1;
                } else {
                    return Ok(block_start + i as u64 + 1);
                }
            }
        }

        remaining -= read_size as u64;
    }

    // Fewer than N+1 newlines total → print the whole file from byte 0.
    Ok(0)
}

fn print_last_lines(file: &mut File, num: i64) -> Result<()> {
    let n = num.unsigned_abs() as usize;
    if n == 0 {
        return Ok(());
    }
    let start = find_last_lines_start(file, n)?;
    file.seek(SeekFrom::Start(start))?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    io::copy(file, &mut out)?;
    Ok(())
}

fn run(args: Args) -> Result<()> {
    let lines = parse_num(args.lines).map_err(|e| anyhow!("illegal line count -- {e}"))?;
    let bytes = args
        .bytes
        .map(parse_num)
        .transpose()
        .map_err(|e| anyhow!("illegal byte count -- {e}"))?;

    let num_files = args.files.len();
    for (file_num, filename) in args.files.iter().enumerate() {
        match File::open(filename) {
            Err(err) => eprintln!("{filename}: {err}"),
            Ok(file) => {
                if !args.quiet && num_files > 1 {
                    println!(
                        "{}==> {} <==",
                        if file_num > 0 { "\n" } else { "" },
                        filename,
                    );
                }
                if let Some(num_bytes) = &bytes {
                    let total_bytes = file.metadata()?.len() as i64;
                    print_bytes(file, num_bytes, total_bytes)?;
                } else {
                    let mut file = file;
                    match &lines {
                        TakeValue::TakeNum(n) if *n < 0 => {
                            print_last_lines(&mut file, *n)?;
                        }
                        _ => {
                            let mut reader = BufReader::new(file);
                            let (total_lines, _) = count_lines_bytes(&mut reader)?;
                            reader.seek(SeekFrom::Start(0))?;
                            print_lines(reader, &lines, total_lines)?;
                        }
                    }
                }
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
    use super::{TakeValue::*, count_lines_bytes, get_start_index, parse_num};
    use crate::{BufReader, File};
    use anyhow::Result;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_count_lines_bytes() -> Result<()> {
        let mut file = BufReader::new(File::open("tests/inputs/one.txt")?);
        let (lines, bytes) = count_lines_bytes(&mut file)?;
        assert_eq!(lines, 1);
        assert_eq!(bytes, 24);

        let mut file = BufReader::new(File::open("tests/inputs/twelve.txt")?);
        let (lines, bytes) = count_lines_bytes(&mut file)?;
        assert_eq!(lines, 12);
        assert_eq!(bytes, 63);
        Ok(())
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
