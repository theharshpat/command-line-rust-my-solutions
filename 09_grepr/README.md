# Chapter 9: `grepr` — Pattern Matching, Recursive Search, and Stream Filtering

`grep` searches input text for lines that match a regular expression and prints the matching lines. The problem it solves is universal in computing: finding needles in text haystacks at scale. Every developer, sysadmin, and data analyst needs it — *which log lines contain an error code?*, *does this file import that module?*, *how many times does a word appear across a project tree?* Without `grep`, every one of those questions needs an ad-hoc script.

The Rust version (`grepr`) mirrors the core POSIX semantics while adding modern CLI conventions via `clap`. It accepts a required pattern, zero-or-more file paths (defaulting to stdin via `-`), compiles the pattern into a `Regex`, iterates over lines, tests each, and either prints matches or counts them.

---

## Prereading

Before this chapter, make sure the following are comfortable. Each shows up implicitly in the design below.

- **Rust's `regex` crate: finite automata, not backtracking** — the engine is linear-time by construction. `Regex::new` / `RegexBuilder::new().build()` are compile steps; `is_match(&str)` is a cheap subsequent call. Knowing *why* this matters (ReDoS / catastrophic backtracking) is the systems-level point of using it.
- **`Regex::new` vs `RegexBuilder`** — `Regex::new` for default options, `RegexBuilder` when any compile-time flag (`case_insensitive`, `multi_line`, `dot_matches_new_line`, `size_limit`, …) depends on runtime input. `grepr` uses `RegexBuilder` because `-i` is a runtime flag.
- **`BufRead` vs `Read`** — `BufRead` gives `read_line(&mut String)`, which appends one line without loading the whole file. A generic `<T: BufRead>` accepts `BufReader<File>`, `BufReader<Stdin>`, and `Cursor<&[u8]>` (used in unit tests) under one signature.
- **`mem::take` and `Default`** — `mem::take(&mut x)` swaps `x` with `Default::default()` and returns the old value by ownership. The canonical "move-out-of-a-mutable-reference" pattern; works for any `Default` type, no clone needed.
- **`Vec<Result<T>>` vs `Result<Vec<T>>`** — the former lets the caller keep processing good entries while reporting bad ones; the latter aborts on the first failure. This chapter's `find_files` returns `Vec<Result<PathBuf>>` precisely so one bad path doesn't kill the whole search.
- **`Iterator::flatten` on `Result` streams** — `Result: IntoIterator`, so `flatten()` on a stream of `Result` silently drops every `Err` (yields zero items). Fine for terse teaching code, lossy for production. The observable alternative is `filter_map` + `eprintln!`.
- **Booleans and XOR** — `a ^ b` on `bool` is `a != b`. A compact idiom for conditional inversion; read `is_match ^ invert` as "match unless inverting."
- **`walkdir::WalkDir` recursion + `file_type().is_file()` filtering** — already seen in ch7 (`findr`); recurs here for `-r`.
- **`clap` derive: required positional, `default_value = "-"`, `short`/`long` flags** — a required positional `pattern: String` needs no `#[arg]`; `files: Vec<PathBuf>` defaults to `-` for stdin.

---

## Requirements

The program must:

1. Accept a required pattern as the first positional argument.
2. Accept zero or more file paths; default to stdin (`-`) when none given.
3. Support `-i` / `--insensitive` for case-insensitive matching.
4. Support `-r` / `--recursive` to descend into directories.
5. Support `-c` / `--count` to print match counts instead of lines.
6. Support `-v` / `--invert-match` to print non-matching lines.
7. Prefix output lines with the filename when searching multiple files.
8. Handle errors gracefully: bad patterns, missing files, directories without `--recursive`, I/O failures.

## The regex engine — linear-time by construction

Rust's `regex` crate uses a finite automaton engine, not backtracking. This guarantees **linear-time matching** with respect to input length — no catastrophic backtracking, the failure mode that has brought down Node.js services, Cloudflare routers, and Stack Overflow itself.

```
// Perl/JS/PCRE:   (a*)*b  on  "aaaaaaaaac"  -> exponential backtracking
// Rust regex:     same pattern              -> O(n) always, no timeout needed
```

This is a *systems property*, not a syntactic one. When you choose Rust's `regex` for a service that matches patterns against untrusted input (URL routing, WAF rules, log scraping), you are choosing a guarantee about worst-case behavior.

### `Regex::new` vs `RegexBuilder`

| API                       | When to use                                  |
|---------------------------|----------------------------------------------|
| `Regex::new("...")`       | Simple patterns, default options             |
| `RegexBuilder::new("...")`| Need non-default flags toggled at runtime    |

`grepr` needs `RegexBuilder` because `case_insensitive` is a runtime flag from `-i`. You could embed `(?i)` in the pattern string, but only if you wanted it always on; `RegexBuilder` lets you flip it based on a `bool`:

```rust
let pattern = RegexBuilder::new(&args.pattern)
    .case_insensitive(args.insensitive)
    .build()
    .map_err(|_| anyhow!(r#"Invalid pattern "{}""#, args.pattern))?;
```

If compilation fails (e.g. `*foo` is invalid), `.build()` returns `Err`, which we map to a user-facing message.

Compare with `findr`'s approach (ch7): there `value_parser(Regex::new)` compiled at clap parse time because there were no runtime flags. Here the flag is runtime, so we compile in `run()` instead.

## File discovery — `Vec<Result>` to keep going past bad paths

`find_files` returns `Vec<Result<PathBuf>>`, not `Result<Vec<PathBuf>>`. The difference is structural: `Result<Vec>` aborts on the first failure; `Vec<Result>` lets the caller keep processing good entries while reporting bad ones.

```
find_files(["foo.txt", "nonexistent.txt", "src/"], false)
  -> [ Ok("foo.txt"),
       Err("nonexistent.txt: No such file or directory"),
       Err("src/ is a directory") ]
```

The algorithm per path:

- If `"-"`, return it verbatim (stdin).
- `fs::metadata(path)`:
  - directory + recursive → walk with `WalkDir`, yield every regular file.
  - directory + not recursive → return an `Err`.
  - file → accept.
  - metadata fails (doesn't exist) → return an `Err`.

### `WalkDir::into_iter().flatten()` silently swallows all errors

`flatten()` works because `Result: IntoIterator` — an `Err` yields zero items, so every traversal error (permission denied, broken symlink, I/O) is silently dropped. The book uses this for brevity. It is fine when the only failures are permission errors on subdirs you don't care about, but it **erases the error signal entirely**.

A more observable pattern is `filter_map` with an `eprintln!` on `Err` — same shape as ch7's `findr`:

```rust
WalkDir::new(path)
    .into_iter()
    .filter_map(|e| match e {
        Err(e)   => { eprintln!("{e}"); None }
        Ok(entry) => Some(entry),
    })
    .filter(|e| e.file_type().is_file())
```

Same results, but errors are visible. For a teaching tool the silent form is acceptable; for a production tool the report-and-continue form is better.

### Why `WalkDir` instead of `std::fs::read_dir`?

| Criterion                 | `WalkDir`                | Manual recursion                  |
|---------------------------|--------------------------|-----------------------------------|
| Depth-first traversal     | Built-in                 | You write the stack               |
| Per-entry error handling  | `flatten()` / `filter_map` | Handle each `Result`              |
| Filtering                 | Method chaining          | Nested loops + conditionals       |
| Cross-platform            | Yes                      | Yes, but more boilerplate         |

For a 40-line function, `WalkDir` is the right tradeoff between control and concision. The pattern recurs in ch7 (`findr`) and ch12 (`fortuner`).

## The line-matching loop

```rust
fn find_lines<T: BufRead>(
    mut file: T,
    pattern: &Regex,
    invert: bool,
) -> Result<Vec<String>> {
    let mut matches = vec![];
    let mut line = String::new();
    loop {
        let bytes = file.read_line(&mut line)?;
        if bytes == 0 { break; }                                   // EOF
        if pattern.is_match(&line) ^ invert {                      // XOR: invert the match decision
            matches.push(mem::take(&mut line));
        }
        line.clear();
    }
    Ok(matches)
}
```

| `is_match` | `invert` | Result                          |
|------------|----------|---------------------------------|
| `true`     | `false`  | `true`  — line included         |
| `false`    | `false`  | `false` — line skipped          |
| `true`     | `true`   | `false` — line skipped (inverted) |
| `false`    | `true`   | `true`  — line included (inverted) |

### Why `BufRead` and not `Read`?

`BufRead` provides `read_line`, which gives line-delimited iteration without loading the whole file into memory. This matters for files that exceed available RAM. The generic `T: BufRead` accepts `BufReader<File>`, `BufReader<Stdin>`, and `Cursor<&[u8]>` (used in unit tests) under a single signature.

### The XOR trick

```rust
pattern.is_match(&line) ^ invert
```

XOR with a `bool` is equivalent to `!=`. It is a compact idiom for conditional inversion that avoids an `if/else` branch. Read it as *"match is true unless we're inverting."*

### `mem::take` — move ownership out, reuse the buffer

```rust
matches.push(mem::take(&mut line));   // line is now "" again, ownership moved
```

`mem::take(&mut line)` replaces `line` with `String::default()` (i.e. `""`) and returns the old `String` by value. The heap allocation of the old line is moved into `matches`; the local `line` binding is left holding an empty `String` ready for the next `read_line` to append into.

Without `mem::take` you would need `line.clone()` — doubling allocation per match. With it, the single buffer is reused across all iterations. This is the canonical "move-out-of-mutable-reference" pattern in Rust, and it works for any type that implements `Default`.

The `line.clear()` afterwards is only needed on the non-match branch (where `mem::take` wasn't called) — `read_line` appends, so without `clear()` the next line would accumulate after the previous one. Both branches end up with `line == ""` for the next iteration, just by different mechanisms.

## Count vs print modes

When `--count` is set, `run` prints the number of matches per file instead of the lines:

```
$ grepr -c The tests/inputs/bustle.txt
3
$ grepr -c The tests/inputs/bustle.txt tests/inputs/fox.txt
tests/inputs/bustle.txt:3
tests/inputs/fox.txt:1
```

The `print` closure captures `num_files` to decide whether to prefix lines with the filename:

```rust
let print = |fname: &str, val: &str| {
    if num_files > 1 { print!("{fname}:{val}"); }
    else             { print!("{val}"); }
};
```

`num_files` is `entries.len()` *before* filtering bad entries — so a bad path that produces an `Err` still counts toward the "multiple files" decision. That is why `grepr fox tests/inputs fox.txt` prefixes `fox.txt:`'s match even though `tests/inputs` produced no output (it errored as "is a directory"). This is intentional and matches the test expectations.

## Streaming vs collect — a contradiction worth noting

The notes for this chapter often praise `BufRead` for not loading the whole file into memory, then immediately call `find_lines` which *does* collect every matching line into a `Vec<String>` before returning. For a file with millions of matches, this defeats the streaming benefit. A `grep` ideally prints matches as they're found:

```rust
fn stream_matches<T: BufRead>(mut file: T, pattern: &Regex, invert: bool) -> Result<()> {
    let mut line = String::new();
    loop {
        let bytes = file.read_line(&mut line)?;
        if bytes == 0 { break; }
        if pattern.is_match(&line) ^ invert {
            print!("{}", line);                 // straight to stdout, no Vec
        }
        line.clear();
    }
    Ok(())
}
```

This is `O(1)` memory in the result set. The book's `Vec<String>` form is simpler to reason about and lets `--count` work as `matches.len()`, but it is the wrong shape for huge files. A modern implementation streams and counts in one pass.

## `grep`'s exit codes — a divergence worth knowing

Real `grep` uses exit codes as a signal: `0` = match found, `1` = no match, `2` = error. This makes `if grep -q pattern file; then ...` work in shell scripts. `grepr` exits `0` on success and `1` on any `Err` — it never signals "no matches" via exit code. For a drop-in `grep` replacement that matters; for a teaching tool it is acceptable.

## Full implementation

```rust
use anyhow::{anyhow, Result};
use clap::Parser;
use regex::{Regex, RegexBuilder};
use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader},
    mem,
    path::PathBuf,
};
use walkdir::WalkDir;

#[derive(Debug, Parser)]
#[command(author, version, about)]
/// Rust version of `grep`
struct Args {
    /// Search pattern
    pattern: String,                                            // <- required positional, no #[arg] needed

    /// Input file(s)
    #[arg(default_value = "-", value_name = "FILE")]
    files: Vec<PathBuf>,                                        // <- PathBuf, not String

    /// Case-insensitive
    #[arg(short, long)]
    insensitive: bool,

    /// Recursive search
    #[arg(short, long)]
    recursive: bool,

    /// Count occurrences (mode switch, not ArgAction::Count)
    #[arg(short, long)]
    count: bool,

    /// Invert match
    #[arg(short('v'), long("invert-match"))]
    invert: bool,
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    // <- RegexBuilder (not Regex::new) because case_insensitive is a runtime flag.
    let pattern = RegexBuilder::new(&args.pattern)
        .case_insensitive(args.insensitive)
        .build()
        .map_err(|_| anyhow!(r#"Invalid pattern "{}""#, args.pattern))?;

    let entries = find_files(&args.files, args.recursive);
    let num_files = entries.len();

    let print = |fname: &str, val: &str| {
        if num_files > 1 { print!("{fname}:{val}"); }
        else             { print!("{val}"); }
    };

    for entry in entries {
        match entry {
            Err(e) => eprintln!("{e}"),
            Ok(filename) => match open(&filename) {
                Err(e) => eprintln!("{}: {e}", filename.display()),
                Ok(file) => match find_lines(file, &pattern, args.invert) {
                    Err(e) => eprintln!("{e}"),
                    Ok(matches) => {
                        if args.count {
                            print(&filename.display().to_string(),
                                  &format!("{}\n", matches.len()));
                        } else {
                            for line in &matches {
                                print(&filename.display().to_string(), line);
                            }
                        }
                    }
                },
            },
        }
    }
    Ok(())
}

fn open(filename: &Path) -> Result<Box<dyn BufRead>> {
    if filename == Path::new("-") {
        Ok(Box::new(BufReader::new(io::stdin())))
    } else {
        Ok(Box::new(BufReader::new(File::open(filename)?)))
    }
}

// <- Generic <T: BufRead> so unit tests can pass Cursor<&[u8]>.
// <- mem::take moves the line's heap allocation into matches without cloning.
// <- XOR with `invert` compactly handles normal and inverted matching in one expr.
fn find_lines<T: BufRead>(
    mut file: T,
    pattern: &Regex,
    invert: bool,
) -> Result<Vec<String>> {
    let mut matches = vec![];
    let mut line = String::new();
    loop {
        let bytes = file.read_line(&mut line)?;
        if bytes == 0 { break; }
        if pattern.is_match(&line) ^ invert {
            matches.push(mem::take(&mut line));
        }
        line.clear();
    }
    Ok(matches)
}

// <- Vec<Result> (not Result<Vec>) so one bad path doesn't abort the whole search.
// <- WalkDir + filter_map + eprintln is the report-and-continue form;
//    the book's .flatten() silently drops all traversal errors.
fn find_files(paths: &[PathBuf], recursive: bool) -> Vec<Result<PathBuf>> {
    let mut results = vec![];
    for path in paths {
        if path == Path::new("-") {
            results.push(Ok(path.clone()));
            continue;
        }
        match fs::metadata(path) {
            Ok(meta) if meta.is_dir() => {
                if recursive {
                    for entry in WalkDir::new(path)
                        .into_iter()
                        .filter_map(|e| match e {
                            Err(e)    => { eprintln!("{e}"); None }
                            Ok(entry) => Some(entry),
                        })
                        .filter(|e| e.file_type().is_file())
                    {
                        results.push(Ok(entry.path().to_path_buf()));
                    }
                } else {
                    results.push(Err(anyhow!("{} is a directory", path.display())));
                }
            }
            Ok(_) => results.push(Ok(path.clone())),
            Err(e) => results.push(Err(anyhow!("{}: {e}", path.display()))),
        }
    }
    results
}
```

## Key takeaways

- **Rust's `regex` crate is linear-time by construction** — finite automaton engine, no catastrophic backtracking. Choosing it for untrusted-input pattern matching is a systems-level safety decision, not a syntax preference.
- **`RegexBuilder` over `Regex::new`** when any compile-time flag depends on runtime input (`case_insensitive`, `multi_line`, `dot_matches_new_line`, etc.).
- **`Vec<Result<T>>` vs `Result<Vec<T>>`** — the former lets processing continue past one bad path; the latter aborts on the first. Choose based on whether "best effort across many inputs" or "all-or-nothing" is the right semantics.
- **`WalkDir::flatten()` silently swallows all traversal errors.** Acceptable for teaching; for production use `filter_map` with `eprintln!` so failures are observable.
- **`BufRead` enables line-delimited streaming without loading the whole file.** Pair it with streaming output (`print!` inside the loop) to keep memory `O(1)` — the book's `Vec<String>` form defeats this for huge result sets.
- **`mem::take(&mut line)`** moves a `String`'s heap allocation out of a mutable reference and replaces it with `""`, enabling zero-copy buffer reuse. Works for any `Default` type.
- **XOR (`a ^ b`) on `bool`s is `a != b`** — a compact conditional-inversion idiom. `is_match ^ invert` reads as "match unless inverting."
- **`grep` exit codes (0/1/2) are not implemented by `grepr`.** For shell `if grep -q ...` workflows this matters; for a teaching tool it is a documented simplification.
