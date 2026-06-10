use assert_cmd::Command;
use pretty_assertions::assert_eq;

#[test]
fn works() {
    assert!(true);
}

// #[test]
// fn does_not_work() {
//     assert!(false);
// }

#[test]
fn runs() {
    let mut cmd = Command::new("ls");
    let res = cmd.output();
    assert!(res.is_ok());
    let res = res.unwrap();
    let output = res.stdout;
    let output_str = String::from_utf8_lossy(&output);
    println!("{:?}", output_str);
}

#[test]
fn non_existent_command_does_not_run() {
    let mut cmd = Command::new("does_not_exist");
    let res = cmd.output();
    assert!(res.is_err());
    // let res = res.unwrap();
    // let output = res.stdout;
    // let output_str = String::from_utf8_lossy(&output);
    // println!("{:?}", output_str);
}

#[test]
fn runs_hello() {
    let mut cmd = Command::cargo_bin("hello").unwrap();

    // runs it once
    cmd.assert().success();

    // runs it again
    let output = cmd.output().unwrap();
    println!("{:?}", output);
}

#[test]
fn runs_hello_tests_output() {
    let mut cmd = Command::cargo_bin("hello").unwrap();
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let output_str = String::from_utf8_lossy(&output.stdout);
    // assert_eq!(output_str, "Bye, world!\n");
    assert_eq!(output_str, "Hello, world!\n");
}
