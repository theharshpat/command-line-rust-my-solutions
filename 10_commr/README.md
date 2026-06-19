# Chapter 10: `commr` — Line-by-Line File Comparison and Columnar Output

The Unix `comm` command compares two sorted files line by line. It produces three columns of output:

- **Column 1** — lines unique to file 1
- **Column 2** — lines unique to file 2
- **Column 3** — lines common to both files

By default all three columns are printed; `-1`, `-2`, `-3` suppress the corresponding column. The output is tab-delimited, though a custom delimiter may be set with `-d`. Either file may be `"-"` (stdin), but not both at once.

---

## Prereading

Before this chapter, make sure the following are comfortable. Each shows up implicitly in the design below.

- **`Ord` / `Ordering` (`Less` / `Equal` / `Greater`) and `cmp`** — the merge loop dispatches on `v1.cmp(&v2)`. `Ordering` is the standard library's three-valued comparison result; pattern-matching on it is the canonical "what does this comparison tell me?" idiom.
- **`Option<T>` and the "prime-then-loop" iterator pattern** — `let mut line1 = lines1.next();` then `while line1.is_some() || line2.is_some() { match (&line1, &line2) { ... } }`. The alternative is `Peekable` (`peek()` / `next()`); the manual state machine is the C-style canonical form.
- **Lifetimes on enums (`Column<'a>`)** — a variant like `Col1(&'a str)` borrows from a caller-owned buffer rather than cloning. The lifetime ties the borrow to the buffer's lifetime; soundness comes from the merge loop structure (borrows end before the next `next()` mutation).
- **`ArgAction::SetFalse`** — inverts clap's boolean default. Normally a `bool` arg defaults to `false` and becomes `true` when present; `SetFalse` flips that, so a `show_col1: bool` field defaults to `true` and becomes `false` when `-1` is passed. Lets call sites read `if args.show_col1` instead of `if !args.suppress_col1`.
- **`Iterator::map_while` + `Result::ok`** — `lines().map_while(Result::ok)` unwraps `Ok` values and silently halts at the first `Err`. Pragmatic for trusted local files, lossy for production (no error surfaced).
- **`BufRead::lines`** — line-delimited iteration without loading the whole file. Yields `io::Result<String>` (one allocation per line, newline stripped).
- **Sort-merge join as an algorithm** — the inner loop of merge sort, and exactly what a relational database does for a join on two pre-sorted inputs. Recognizing this shape is the point of the chapter if you care about data systems.
- **`Path` / `PathBuf`** — positional file args are `PathBuf`, not `String`; `"-"` is compared via `Path::new("-")`.

---

## What problem does `comm` solve?

This is fundamentally a **sort-merge join** algorithm: walk two sorted sequences in lockstep, emitting a record for every comparison outcome. No sorting is performed — `comm` *assumes* both inputs are already sorted. If they aren't, the output is garbage, which is why `comm` in practice is almost always preceded by `sort`.

```
// merge state machine:
//   val1 <  val2  ->  col1, advance file1
//   val1 == val2  ->  col3, advance both
//   val1 >  val2  ->  col2, advance file2
```

This is exactly the inner loop of merge sort, and exactly the algorithm a relational database uses for a sort-merge join on two pre-sorted inputs. Once you see `comm` as *"merge-join with three output sinks,"* the same pattern recurs in stream processors, `diff` engines, and replication lag monitors.

```
   file1 ──► next() ──┐
                     │
                     ▼
              ┌───────────────┐
   file2 ──►  │   compare     │   val1.cmp(val2) -> Ordering
        next()│               │
              └──┬──────┬─────┘
                 │      │
            Less │      │ Equal      │ Greater
                 ▼      ▼            ▼
               col1   col3          col2
               push   push          push
               adv1   adv1+2        adv2
```

## Requirements

- Accept two positional file arguments; `"-"` means stdin.
- Allow `-1`, `-2`, `-3` to suppress the corresponding column (default: all shown).
- Accept `-i` for case-insensitive comparison.
- Accept `-d` / `--output-delimiter` (default: tab).
- Both files cannot simultaneously be stdin.
- Produce three tab-separated columns: col1 un-prefixed, col2 prefixed by one tab, col3 prefixed by two tabs (when all three shown).
- Skip rows that would produce an empty output (all visible columns suppressed).

## The merge algorithm

The core insight is that `comm` performs a **three-way merge step** at every iteration. Given two sorted iterators, at each step we look at the current line from each file (if any) and compare them via `Ord`:

```
Line1 <  Line2  ->  print Line1 in column 1, advance file 1
Line1 == Line2  ->  print Line1 in column 3, advance both
Line1 >  Line2  ->  print Line2 in column 2, advance file 2
```

When one file is exhausted, the remaining lines from the other all go into that file's column. The structural logic is identical regardless of the comparison function — case-insensitive comparison just swaps the `Ord` implementation, not the merge.

## The `Column<'a>` enum — borrowing instead of allocating

The code defines a small enum to carry the result of each comparison to the printer:

```rust
enum Column<'a> {
    Col1(&'a str),   // unique to file 1
    Col2(&'a str),   // unique to file 2
    Col3(&'a str),   // common to both
}
```

The lifetime parameter lets each variant borrow a `str` slice from the line buffer instead of cloning an owned `String`:

```
// Without 'a:  Col1(String)  ->  alloc per comparison (clone)
// With 'a:     Col1(&str)    ->  borrow from the line buffer (zero-copy handoff)
```

This is sound because of how the merge loop is structured: `line1` and `line2` are owned `Option<String>`s in local bindings; the `match` takes shared references into them; the `print` closure runs synchronously and consumes the references *before* the next `lines.next()` assignment executes. The borrow ends before any mutation. No `unsafe`, no self-referential borrow, no escaping references.

A simpler alternative — `enum Column { Col1(String), ... }` moving the owned string — would also work and avoid the lifetime juggling, at the cost of one move per comparison. For a teaching example the lifetime version is a fine illustration of Rust's zero-copy capabilities; for production the owned version is often clearer and the cost is negligible.

## The print closure — tab alignment via placeholders

The interesting part is how the closure maintains column alignment when columns are suppressed:

```rust
let print = |col: Column| {
    let mut columns: Vec<&str> = vec![];
    match col {
        Col1(val) => if args.show_col1 { columns.push(val); },
        Col2(val) => if args.show_col2 {
            if args.show_col1 { columns.push(""); }   // placeholder for col1
            columns.push(val);
        },
        Col3(val) => if args.show_col3 {
            if args.show_col1 { columns.push(""); }   // placeholder for col1
            if args.show_col2 { columns.push(""); }   // placeholder for col2
            columns.push(val);
        },
    }
    if !columns.is_empty() {
        println!("{}", columns.join(&args.delimiter));
    }
};
```

The rule: **for column N being emitted, prepend one empty-string placeholder for each visible preceding column.** Suppressed preceding columns contribute no placeholder. This keeps the position of each column in the output line consistent regardless of which columns are visible.

| col1 shown? | col2 shown? | col3 shown? | Placeholder behavior for the emitted column |
|-------------|-------------|-------------|----------------------------------------------|
| yes         | yes         | yes         | col1: none; col2: 1; col3: 2                 |
| **no**      | yes         | yes         | col2: 0 (col1 suppressed → no placeholder)   |
| yes         | **no**      | yes         | col3: 1 (col2 suppressed → no placeholder)   |
| **no**      | **no**      | yes         | col3: 0 (both suppressed → no placeholder)   |

Concrete examples from the test fixtures (`file1 = a,b,c,d`; `file2 = B,c`):

```
$ commr file1 file2      # all three shown
B
a
b
        c
d

$ commr -1 file1 file2   # suppress col1
        b
        c

$ commr -123 file1 file2 # suppress all -> empty output
```

The `if !columns.is_empty()` guard is what makes `-123` (all suppressed) produce no output at all — without it, every row would print a blank line.

## Case-insensitive comparison — a fidelity bug worth fixing

The book defines a closure that folds lines to lowercase before they enter the iterator:

```rust
let case = |line: String| {
    if args.insensitive { line.to_lowercase() } else { line }
};

let mut lines1 = open(file1).lines().map_while(Result::ok).map(case);
let mut lines2 = open(file2).lines().map_while(Result::ok).map(case);
```

The folded string is then used for **both comparison and printing**. Consequence: under `comm -i`, a line `B` in `file2` that matches `b` in `file1` is printed as `b`, not `B`. The original casing is lost.

This is *not* what POSIX `comm -i` does. GNU `comm -i` compares case-insensitively but emits the original line text. The book's simplification prints the lowercased text — a real fidelity bug. The test golden file `file1_file2.1.i.out` contains `\tb\n\tc\n`, where the `b` was originally `B` in `file2.txt`.

The fix is to carry both forms through the pipeline — compare on the folded version, print the original:

```rust
// <- Modernization: preserve original casing under -i.
// Map each line to (original, key) where `key` is the case-folded form
// used for comparison; print the original.
let case = |line: String| -> (String, String) {
    if args.insensitive {
        (line.clone(), line.to_lowercase())
    } else {
        (line.clone(), line)
    }
};

let mut lines1 = open(file1).lines().map_while(Result::ok).map(case);
let mut lines2 = open(file2).lines().map_while(Result::ok).map(case);

// In the merge loop, compare .1 and pass .0 to print:
match (&line1, &line2) {
    (Some(v1), Some(v2)) => match v1.1.cmp(&v2.1) {     // compare keys
        Equal   => { print(Col3(&v1.0));                 // print originals
                      line1 = lines1.next();
                      line2 = lines2.next(); }
        Less    => { print(Col1(&v1.0)); line1 = lines1.next(); }
        Greater => { print(Col2(&v2.0)); line2 = lines2.next(); }
    },
    ...
}
```

This makes the output match GNU `comm -i` exactly — case-insensitive comparison, original-case emission. The cost is one extra `String` per line under `-i`; negligible for typical `comm` use.

## The while-match state machine

The merge loop is a classic state-machine iteration pattern:

```rust
let mut line1 = lines1.next();        // prime both cursors
let mut line2 = lines2.next();

while line1.is_some() || line2.is_some() {
    match (&line1, &line2) {
        (Some(v1), Some(v2)) => match v1.1.cmp(&v2.1) {
            Equal   => { print(Col3(&v1.0)); line1 = lines1.next(); line2 = lines2.next(); }
            Less    => { print(Col1(&v1.0)); line1 = lines1.next(); }
            Greater => { print(Col2(&v2.0)); line2 = lines2.next(); }
        },
        (Some(v1), None)     => { print(Col1(&v1.0)); line1 = lines1.next(); }
        (None, Some(v2))     => { print(Col2(&v2.0)); line2 = lines2.next(); }
        _                    => (),     // both None — loop guard already excludes this
    }
}
```

Four states cover the space:

```
// (line1, line2) states:
//   (Some, Some)  ->  compare and dispatch
//   (Some, None)  ->  drain file1 into col1
//   (None, Some)  ->  drain file2 into col2
//   (None, None)  ->  done (unreachable: while guard)
```

The pattern works because `Option<String>` values are moved out of the iterator and re-assigned each iteration; the borrows inside the `match` are temporary and end before the next `next()` call. The `_ => ()` arm exists only for exhaustiveness — the loop guard guarantees it never runs.

An alternative using `Peekable` (`lines.peek()` / `next()`) avoids the prime-then-loop dance but is structurally similar. `itertools::merge_join_by` would express the whole thing as one iterator combinator yielding `EitherOrBoth` — more declarative, less pedagogical. The manual state machine is the canonical C-style merge and is worth recognizing.

## `ArgAction::SetFalse` — inverting the boolean default

The `-1`, `-2`, `-3` flags use `ArgAction::SetFalse`:

```rust
/// Suppress printing of column 1
#[arg(short('1'), action(ArgAction::SetFalse))]
show_col1: bool,
```

Normally a boolean `#[arg(short)]` defaults to `false` and is `true` when present. But the semantics here are inverted: the flag *suppresses* a column that is shown by default. `SetFalse` flips the default to `true` and sets the field to `false` when the flag is present:

```
flag present  ->  show_col1 = false  (column suppressed)
flag absent   ->  show_col1 = true   (column shown)
```

This lets the rest of the code read `if args.show_col1` naturally, rather than `if !args.suppress_col1`. The field name describes the *positive* behavior, which is clearer at every read site.

`short('1')` is needed because clap does not auto-infer digit characters as short flags — only letters. Explicit binding is required.

## `map_while(Result::ok)` — silent error halt

```rust
.lines().map_while(Result::ok).map(case)
```

`lines()` yields `io::Result<String>`. `map_while(Result::ok)` unwraps `Ok` values and stops the iterator at the first `Err`, silently dropping the error. For trusted local files this is pragmatic — a mid-file read error is rare. For a production tool you'd want to surface read failures via `filter_map` + logging, or eager collection with `?` propagation. The pattern recurs in ch12 (`fortuner`); it is a deliberate "good enough" choice, not a best practice.

## Full implementation

```rust
use crate::Column::*;
use anyhow::{anyhow, bail, Result};
use clap::{ArgAction, Parser};
use std::{
    cmp::Ordering::*,
    fs::File,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
#[command(author, version, about)]
/// Rust version of `comm`
struct Args {
    /// Input file 1
    file1: PathBuf,                                              // <- PathBuf, not String

    /// Input file 2
    file2: PathBuf,

    /// Suppress printing of column 1
    #[arg(short('1'), action(ArgAction::SetFalse))]
    show_col1: bool,                                             // <- inverted default

    /// Suppress printing of column 2
    #[arg(short('2'), action(ArgAction::SetFalse))]
    show_col2: bool,

    /// Suppress printing of column 3
    #[arg(short('3'), action(ArgAction::SetFalse))]
    show_col3: bool,

    /// Case-insensitive comparison
    #[arg(short, long("ignore-case"))]
    insensitive: bool,                                           // <- add long alias GNU has

    /// Output delimiter
    #[arg(short, long("output-delimiter"), default_value = "\t")]
    delimiter: String,
}

enum Column<'a> {
    Col1(&'a str),
    Col2(&'a str),
    Col3(&'a str),
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    if args.file1 == Path::new("-") && args.file2 == Path::new("-") {
        bail!(r#"Both input files cannot be STDIN ("-")"#);
    }

    // <- Modernization: carry (original, key) so -i compares case-insensitively
    //    but prints the original line. The book lowercased the line and lost
    //    the original casing — a divergence from POSIX comm -i.
    let case = |line: String| -> (String, String) {
        if args.insensitive {
            (line.clone(), line.to_lowercase())
        } else {
            (line.clone(), line)
        }
    };

    let mut lines1 = open(&args.file1).lines()
        .map_while(Result::ok)
        .map(case);
    let mut lines2 = open(&args.file2).lines()
        .map_while(Result::ok)
        .map(case);

    let print = |col: Column| {
        let mut columns: Vec<&str> = vec![];
        match col {
            Col1(val) => if args.show_col1 { columns.push(val); },
            Col2(val) => if args.show_col2 {
                if args.show_col1 { columns.push(""); }         // placeholder for col1
                columns.push(val);
            },
            Col3(val) => if args.show_col3 {
                if args.show_col1 { columns.push(""); }         // placeholder for col1
                if args.show_col2 { columns.push(""); }         // placeholder for col2
                columns.push(val);
            },
        }
        if !columns.is_empty() {
            println!("{}", columns.join(&args.delimiter));
        }
    };

    let mut line1 = lines1.next();
    let mut line2 = lines2.next();
    while line1.is_some() || line2.is_some() {
        match (&line1, &line2) {
            (Some(v1), Some(v2)) => match v1.1.cmp(&v2.1) {     // compare keys
                Equal   => { print(Col3(&v1.0));                 // print originals
                              line1 = lines1.next();
                              line2 = lines2.next(); }
                Less    => { print(Col1(&v1.0)); line1 = lines1.next(); }
                Greater => { print(Col2(&v2.0)); line2 = lines2.next(); }
            },
            (Some(v1), None)     => { print(Col1(&v1.0)); line1 = lines1.next(); }
            (None, Some(v2))     => { print(Col2(&v2.0)); line2 = lines2.next(); }
            _                    => (),
        }
    }
    Ok(())
}

fn open(filename: &Path) -> Result<Box<dyn BufRead>> {
    if filename == Path::new("-") {
        Ok(Box::new(BufReader::new(io::stdin())))
    } else {
        Ok(Box::new(BufReader::new(
            File::open(filename).map_err(|e| anyhow!("{}: {e}", filename.display()))?,
        )))
    }
}
```

## Key takeaways

- **`comm` is a sort-merge join with three output sinks.** The same algorithm underlies merge sort's inner loop, relational database joins on sorted inputs, and the Unix `join` command.
- **The while-`match` on a tuple of `Option` is the canonical state-machine idiom** for walking two iterators in lockstep. Four states cover the space: both present, only left, only right, both exhausted.
- **`ArgAction::SetFalse` inverts boolean defaults.** Use it when a flag means "disable," so the field reads as the positive behavior (`show_col1: true`) rather than a negation (`suppress_col1: false`). `short('1')` is explicit because clap won't auto-infer digit shorts.
- **Tab alignment via empty-string placeholders** keeps column positions consistent when columns are suppressed. The rule: one placeholder per visible preceding column.
- **`Column<'a>` borrowing from line buffers is sound** because the merge loop's borrows end before any mutation. Zero-copy handoff to the printer; the cost is lifetime juggling.
- **`map_while(Result::ok)` silently halts at the first I/O error.** Acceptable for trusted local files; for production, `filter_map` + logging or eager `?` propagation is more observable.
- **The book's `-i` implementation is a fidelity bug vs POSIX.** It lowercases lines for both comparison and printing; GNU `comm -i` compares case-insensitively but emits original case. The modern form carries `(original, key)` tuples and compares on `.1` while printing `.0`.
