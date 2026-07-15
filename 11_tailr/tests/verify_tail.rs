use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn input_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("verify")
        .join("inputs")
}

fn inputs() -> Vec<String> {
    let dir = input_dir();
    let mut files: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {:?}: {e}", dir))
        .map(|e| e.unwrap().path().to_string_lossy().to_string())
        .collect();
    files.sort();
    files
}

fn tailr(args: &[&str], filename: &str) -> Vec<u8> {
    let cmd = Command::new(env!("CARGO_BIN_EXE_tailr"))
        .args(args)
        .arg(filename)
        .output()
        .unwrap_or_else(|e| panic!("tailr: {e}"));
    cmd.stdout
}

fn bsd_tail(args: &[&str], filename: &str) -> Vec<u8> {
    let cmd = Command::new("tail")
        .args(args)
        .arg(filename)
        .output()
        .unwrap_or_else(|e| panic!("tail: {e}"));
    cmd.stdout
}

fn check(filename: &str, args: &[&str]) {
    let expected = bsd_tail(args, filename);
    let actual = tailr(args, filename);
    assert_eq!(
        actual, expected,
        "\nfile: {filename}\nargs: {args:?}\nexpected ({} bytes): {:?}\nactual   ({} bytes): {:?}",
        expected.len(),
        String::from_utf8_lossy(&expected),
        actual.len(),
        String::from_utf8_lossy(&actual),
    );
}

const LINE_COUNTS: &[&str] = &[
    // default (last 10) - covered separately since tail uses no flag
    "-n 1",
    "-n 2",
    "-n 3",
    "-n 5",
    "-n 10",
    "-n 100",
    "-n 1000",
    "-n 0",
    "-n +0",
    "-n +1",
    "-n +2",
    "-n +3",
    "-n +100",
    "-n +1000",
];

const BYTE_COUNTS: &[&str] = &[
    "-c 0",
    "-c 1",
    "-c 2",
    "-c 5",
    "-c 10",
    "-c 100",
    "-c 1000",
    "-c +0",
    "-c +1",
    "-c +2",
    "-c +100",
];

#[test]
fn matches_tail_default_all_files() {
    for f in inputs() {
        check(&f, &[]);
    }
}

#[test]
fn matches_tail_line_counts_all_files() {
    for f in inputs() {
        for n in LINE_COUNTS {
            check(&f, &[n.split_whitespace().collect::<Vec<_>>().as_slice()].concat());
        }
    }
}

#[test]
fn matches_tail_byte_counts_all_files() {
    for f in inputs() {
        for c in BYTE_COUNTS {
            check(&f, &[c.split_whitespace().collect::<Vec<_>>().as_slice()].concat());
        }
    }
}