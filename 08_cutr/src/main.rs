use anyhow::{Result, bail};
use clap::Parser;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
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

fn parse_pos() {
    unimplemented!()
}

fn extract_fields() {
    unimplemented!()
}

fn extract_bytes() {
    unimplemented!()
}

fn extract_chars() {
    unimplemented!()
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
    println!("delimiter byte: {}", delimiter_byte);

    Ok(())
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

// #[cfg(test)]
// mod unit_tests {
//     use super::{extract_bytes, extract_chars, extract_fields, parse_pos};
//     use csv::StringRecord;
//     use pretty_assertions::assert_eq;

//     #[test]
//     fn test_parse_pos() {
//         // The empty string is an error
//         assert!(parse_pos("".to_string()).is_err());

//         // Zero is an error
//         let res = parse_pos("0".to_string());
//         assert!(res.is_err());
//         assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "0""#);

//         let res = parse_pos("0-1".to_string());
//         assert!(res.is_err());
//         assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "0""#);

//         // A leading "+" is an error
//         let res = parse_pos("+1".to_string());
//         assert!(res.is_err());
//         assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "+1""#,);

//         let res = parse_pos("+1-2".to_string());
//         assert!(res.is_err());
//         assert_eq!(
//             res.unwrap_err().to_string(),
//             r#"illegal list value: "+1-2""#,
//         );

//         let res = parse_pos("1-+2".to_string());
//         assert!(res.is_err());
//         assert_eq!(
//             res.unwrap_err().to_string(),
//             r#"illegal list value: "1-+2""#,
//         );

//         // Any non-number is an error
//         let res = parse_pos("a".to_string());
//         assert!(res.is_err());
//         assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "a""#);

//         let res = parse_pos("1,a".to_string());
//         assert!(res.is_err());
//         assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "a""#);

//         let res = parse_pos("1-a".to_string());
//         assert!(res.is_err());
//         assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "1-a""#,);

//         let res = parse_pos("a-1".to_string());
//         assert!(res.is_err());
//         assert_eq!(res.unwrap_err().to_string(), r#"illegal list value: "a-1""#,);

//         // Wonky ranges
//         let res = parse_pos("-".to_string());
//         assert!(res.is_err());

//         let res = parse_pos(",".to_string());
//         assert!(res.is_err());

//         let res = parse_pos("1,".to_string());
//         assert!(res.is_err());

//         let res = parse_pos("1-".to_string());
//         assert!(res.is_err());

//         let res = parse_pos("1-1-1".to_string());
//         assert!(res.is_err());

//         let res = parse_pos("1-1-a".to_string());
//         assert!(res.is_err());

//         // First number must be less than second
//         let res = parse_pos("1-1".to_string());
//         assert!(res.is_err());
//         assert_eq!(
//             res.unwrap_err().to_string(),
//             "First number in range (1) must be lower than second number (1)"
//         );

//         let res = parse_pos("2-1".to_string());
//         assert!(res.is_err());
//         assert_eq!(
//             res.unwrap_err().to_string(),
//             "First number in range (2) must be lower than second number (1)"
//         );

//         // All the following are acceptable
//         let res = parse_pos("1".to_string());
//         assert!(res.is_ok());
//         assert_eq!(res.unwrap(), vec![0..1]);

//         let res = parse_pos("01".to_string());
//         assert!(res.is_ok());
//         assert_eq!(res.unwrap(), vec![0..1]);

//         let res = parse_pos("1,3".to_string());
//         assert!(res.is_ok());
//         assert_eq!(res.unwrap(), vec![0..1, 2..3]);

//         let res = parse_pos("001,0003".to_string());
//         assert!(res.is_ok());
//         assert_eq!(res.unwrap(), vec![0..1, 2..3]);

//         let res = parse_pos("1-3".to_string());
//         assert!(res.is_ok());
//         assert_eq!(res.unwrap(), vec![0..3]);

//         let res = parse_pos("0001-03".to_string());
//         assert!(res.is_ok());
//         assert_eq!(res.unwrap(), vec![0..3]);

//         let res = parse_pos("1,7,3-5".to_string());
//         assert!(res.is_ok());
//         assert_eq!(res.unwrap(), vec![0..1, 6..7, 2..5]);

//         let res = parse_pos("15,19-20".to_string());
//         assert!(res.is_ok());
//         assert_eq!(res.unwrap(), vec![14..15, 18..20]);
//     }

//     #[test]
//     fn test_extract_fields() {
//         let rec = StringRecord::from(vec!["Captain", "Sham", "12345"]);
//         assert_eq!(extract_fields(&rec, &[0..1]), &["Captain"]);
//         assert_eq!(extract_fields(&rec, &[1..2]), &["Sham"]);
//         assert_eq!(extract_fields(&rec, &[0..1, 2..3]), &["Captain", "12345"]);
//         assert_eq!(extract_fields(&rec, &[0..1, 3..4]), &["Captain"]);
//         assert_eq!(extract_fields(&rec, &[1..2, 0..1]), &["Sham", "Captain"]);
//     }

//     #[test]
//     fn test_extract_chars() {
//         assert_eq!(extract_chars("", &[0..1]), "".to_string());
//         assert_eq!(extract_chars("ábc", &[0..1]), "á".to_string());
//         assert_eq!(extract_chars("ábc", &[0..1, 2..3]), "ác".to_string());
//         assert_eq!(extract_chars("ábc", &[0..3]), "ábc".to_string());
//         assert_eq!(extract_chars("ábc", &[2..3, 1..2]), "cb".to_string());
//         assert_eq!(extract_chars("ábc", &[0..1, 1..2, 4..5]), "áb".to_string());
//     }

//     #[test]
//     fn test_extract_bytes() {
//         assert_eq!(extract_bytes("ábc", &[0..1]), "�".to_string());
//         assert_eq!(extract_bytes("ábc", &[0..2]), "á".to_string());
//         assert_eq!(extract_bytes("ábc", &[0..3]), "áb".to_string());
//         assert_eq!(extract_bytes("ábc", &[0..4]), "ábc".to_string());
//         assert_eq!(extract_bytes("ábc", &[3..4, 2..3]), "cb".to_string());
//         assert_eq!(extract_bytes("ábc", &[0..2, 5..6]), "á".to_string());
//     }
// }
