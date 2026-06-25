use anyhow::{Result, anyhow, bail};
use clap::Parser;
use csv::StringRecord;
use regex::Regex;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::num::NonZeroUsize;
use std::ops::Range;

#[derive(Debug, Parser)]
#[command(version, about = "Rust version of `cut`")]
struct Args {
    /// Input file(s)
    #[arg(value_name = "FILES", default_value = "-", num_args = 0..)]
    files: Vec<String>,

    /// Field delimiter
    #[arg(short, long, value_name = "DELIMITER", default_value = "\t")]
    delimiter: String,

    #[command(flatten)]
    extract: ArgsExtract,
}

#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]
struct ArgsExtract {
    /// Selected fields
    #[arg(short, long, value_name = "FIELDS")]
    fields: Option<String>,

    /// Selected bytes
    #[arg(short, long, value_name = "BYTES")]
    bytes: Option<String>,

    /// Selected chars
    #[arg(short, long, value_name = "CHARS")]
    chars: Option<String>,
}

#[derive(Debug)]
enum Extract {
    Fields(Vec<Range<usize>>),
    Bytes(Vec<Range<usize>>),
    Chars(Vec<Range<usize>>),
}

fn parse_index(input: &str) -> Result<usize> {
    if input.starts_with('+') {
        bail!(r#"illegal list value: "{input}""#);
    }
    input
        .parse::<NonZeroUsize>()
        .map(|n| usize::from(n) - 1)
        .map_err(|_| anyhow!(r#"illegal list value: "{input}""#))
}

fn parse_pos(range: String) -> Result<Vec<Range<usize>>> {
    let range_re = Regex::new(r"^(\d+)-(\d+)$").unwrap();
    range
        .split(',')
        .map(|seg| {
            if let Ok(n) = parse_index(seg) {
                return Ok(n..n + 1);
            }
            if let Some(caps) = range_re.captures(seg) {
                let n1 = parse_index(&caps[1])?;
                let n2 = parse_index(&caps[2])?;
                if n1 >= n2 {
                    bail!(
                        "First number in range ({}) must be lower than second number ({})",
                        n1 + 1,
                        n2 + 1
                    );
                }
                return Ok(n1..n2 + 1);
            }
            bail!(r#"illegal list value: "{seg}""#)
        })
        .collect()
}

fn extract_fields(line: &str, field_pos: &[Range<usize>], delimiter_byte: u8) -> String {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter_byte)
        .has_headers(false)
        .from_reader(line.as_bytes());
    let record = reader.records().next().unwrap().unwrap();
    let mut fields = Vec::new();
    for range in field_pos {
        for i in range.clone() {
            if let Some(field) = record.get(i) {
                fields.push(field);
            }
        }
    }
    fields.join(&(delimiter_byte as char).to_string())
}

fn extract_bytes(line: &str, byte_pos: &[Range<usize>]) -> String {
    let bytes = line.as_bytes();
    let mut selected: Vec<u8> = Vec::new();
    for range in byte_pos {
        for i in range.clone() {
            if let Some(&byte) = bytes.get(i) {
                selected.push(byte);
            }
        }
    }
    String::from_utf8_lossy(&selected).into_owned()
}

fn extract_chars(line: &str, char_pos: &[Range<usize>]) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut selected: Vec<char> = Vec::new();
    for range in char_pos {
        for i in range.clone() {
            if let Some(&ch) = chars.get(i) {
                selected.push(ch);
            }
        }
    }
    selected.into_iter().collect()
}

fn open(filename: &str) -> Result<Box<dyn BufRead>> {
    match filename {
        "-" => Ok(Box::new(BufReader::new(io::stdin()))),
        _ => Ok(Box::new(BufReader::new(File::open(filename)?))),
    }
}

fn run(args: Args) -> Result<()> {
    if args.delimiter.len() != 1 {
        bail!(r#"--delimiter "{}" must be a single byte"#, args.delimiter);
    }
    let delimiter_byte = args.delimiter.as_bytes()[0];

    let extract = if let Some(fields) = args.extract.fields {
        Extract::Fields(parse_pos(fields)?)
    } else if let Some(bytes) = args.extract.bytes {
        Extract::Bytes(parse_pos(bytes)?)
    } else if let Some(chars) = args.extract.chars {
        Extract::Chars(parse_pos(chars)?)
    } else {
        unreachable!("clap #[group(required, multiple = false)] guarantees one is set")
    };

    for filename in &args.files {
        let mut reader = open(&filename)?;

        for line in reader.lines() {
            let line = line?;
            match &extract {
                Extract::Fields(field_pos) => {
                    println!("{}", extract_fields(&line, field_pos, delimiter_byte));
                }
                Extract::Bytes(byte_pos) => {
                    println!("{}", extract_bytes(&line, byte_pos));
                }
                Extract::Chars(char_pos) => {
                    println!("{}", extract_chars(&line, char_pos));
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
mod unit_tests {
    use super::{extract_bytes, extract_chars, extract_fields, parse_pos};
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_pos() {
        // The empty string is an error
        assert!(parse_pos("".to_string()).is_err());

        // Zero is an error
        let res = parse_pos("0".to_string());
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "0""#);

        let res = parse_pos("0-1".to_string());
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "0""#);

        // A leading "+" is an error
        let res = parse_pos("+1".to_string());
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "+1""#,);

        let res = parse_pos("+1-2".to_string());
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            r#"illegal list value: "+1-2""#,
        );

        let res = parse_pos("1-+2".to_string());
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            r#"illegal list value: "1-+2""#,
        );

        // Any non-number is an error
        let res = parse_pos("a".to_string());
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "a""#);

        let res = parse_pos("1,a".to_string());
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "a""#);

        let res = parse_pos("1-a".to_string());
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "1-a""#,);

        let res = parse_pos("a-1".to_string());
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "a-1""#,);

        // Wonky ranges
        let res = parse_pos("-".to_string());
        assert!(res.is_err());

        let res = parse_pos(",".to_string());
        assert!(res.is_err());

        let res = parse_pos("1,".to_string());
        assert!(res.is_err());

        let res = parse_pos("1-".to_string());
        assert!(res.is_err());

        let res = parse_pos("1-1-1".to_string());
        assert!(res.is_err());

        let res = parse_pos("1-1-a".to_string());
        assert!(res.is_err());

        // First number must be less than second
        let res = parse_pos("1-1".to_string());
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            "First number in range (1) must be lower than second number (1)"
        );

        let res = parse_pos("2-1".to_string());
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            "First number in range (2) must be lower than second number (1)"
        );

        // All the following are acceptable
        let res = parse_pos("1".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), vec![0..1]);

        let res = parse_pos("01".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), vec![0..1]);

        let res = parse_pos("1,3".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), vec![0..1, 2..3]);

        let res = parse_pos("001,0003".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), vec![0..1, 2..3]);

        let res = parse_pos("1-3".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), vec![0..3]);

        let res = parse_pos("0001-03".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), vec![0..3]);

        let res = parse_pos("1,7,3-5".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), vec![0..1, 6..7, 2..5]);

        let res = parse_pos("15,19-20".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), vec![14..15, 18..20]);
    }

    #[test]
    fn test_extract_fields() {
        let line = "Captain\tSham\t12345";
        assert_eq!(extract_fields(line, &[0..1], b'\t'), "Captain".to_string());
        assert_eq!(extract_fields(line, &[1..2], b'\t'), "Sham".to_string());
        assert_eq!(
            extract_fields(line, &[0..1, 2..3], b'\t'),
            "Captain\t12345".to_string()
        );
        assert_eq!(extract_fields(line, &[0..1, 3..4], b'\t'), "Captain".to_string());
        assert_eq!(
            extract_fields(line, &[1..2, 0..1], b'\t'),
            "Sham\tCaptain".to_string()
        );
    }

    #[test]
    fn test_extract_chars() {
        assert_eq!(extract_chars("", &[0..1]), "".to_string());
        assert_eq!(extract_chars("ábc", &[0..1]), "á".to_string());
        assert_eq!(extract_chars("ábc", &[0..1, 2..3]), "ác".to_string());
        assert_eq!(extract_chars("ábc", &[0..3]), "ábc".to_string());
        assert_eq!(extract_chars("ábc", &[2..3, 1..2]), "cb".to_string());
        assert_eq!(extract_chars("ábc", &[0..1, 1..2, 4..5]), "áb".to_string());
    }

    #[test]
    fn test_extract_bytes() {
        assert_eq!(extract_bytes("ábc", &[0..1]), "�".to_string());
        assert_eq!(extract_bytes("ábc", &[0..2]), "á".to_string());
        assert_eq!(extract_bytes("ábc", &[0..3]), "áb".to_string());
        assert_eq!(extract_bytes("ábc", &[0..4]), "ábc".to_string());
        assert_eq!(extract_bytes("ábc", &[3..4, 2..3]), "cb".to_string());
        assert_eq!(extract_bytes("ábc", &[0..2, 5..6]), "á".to_string());
    }
}
