use anyhow::Result;
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

fn run(args: &[&str], expected_file: &str) -> Result<()> {
    let expected = fs::read_to_string(expected_file)?;
    let mut cmd = Command::cargo_bin("echor")?;
    cmd.args(args).assert().success().stdout(expected);
    Ok(())
}

#[test]
fn dies_no_args() -> Result<()> {
    let mut cmd = Command::cargo_bin("echor")?;
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
    Ok(())
}

#[test]
fn hello1() -> Result<()> {
    run(&["Hello there"], "tests/expected/hello1.txt")
}

#[test]
fn hello2() -> Result<()> {
    run(&["Hello", "there"], "tests/expected/hello2.txt")
}

#[test]
fn hello1_n() -> Result<()> {
    run(&["-n", "Hello  there"], "tests/expected/hello1.n.txt")
}

#[test]
fn hello2_n() -> Result<()> {
    run(&["-n", "Hello", "there"], "tests/expected/hello2.n.txt")
}
