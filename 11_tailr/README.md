# Chapter 11: `tailr` — End-of-File Reading, Seeking, and Negative Offsets

The POSIX `tail` utility prints the end of a file. Its core design question is: *how do you select which portion of a file to emit?* The answer is a two-axis system:

1. **What unit to count** — lines (`-n`) or bytes (`-c`)
2. **Which direction to count from** — from the end (negative / implicit) or from the start (with `+`)

| Argument | Meaning              | Example              |
|----------|----------------------|----------------------|
| `-n 10`  | Last 10 lines (default) | `tail -n 10 foo.txt` |
| `-n +3`  | From line 3 to end   | `tail -n +3 foo.txt` |
| `-c 100` | Last 100 bytes       | `tail -c 100 foo.txt`|
| `-c +50` | From byte 50 to end  | `tail -c +50 foo.txt`|

The critical insight is that a bare number like `10` is *negative* (from the end), while `+10` is *positive* (from the start). This is the opposite of what most programmers expect, and it is the core design challenge of the utility.

---

## Prereading

Before this chapter, make sure the following are comfortable. Each shows up implicitly in the design below.

- **`Read` vs `BufRead` vs `Seek` traits** — `Read` is byte-level; `BufRead` adds `read_until`/`read_line` (buffered, delimiter-aware); `Seek` adds `seek(SeekFrom::Start(u64) | End(i64) | Current(i64))` for random access. Trait bounds like `<T: Read + Seek>` express "I need both" at the type level. `tail`'s byte mode needs `Read + Seek`; its line mode needs `BufRead`.
- **`i64` arithmetic at the boundary, `wrapping_neg`, and overflow** — `-i64::MIN` would be `i64::MAX + 1`, which overflows (panic in debug, wrap in release). `i64::MIN.wrapping_neg()` returns `i64::MIN` by design — the only safe negation at the type boundary. Any time you negate arbitrary `i64` input, reach for `wrapping_neg`.
- **`Option<T>` as "no output" vs "start here"** — `get_start_index` returns `Option<u64>` where `None` means "emit nothing" and `Some(offset)` means "begin at this byte/line." This collapses the "empty result" case into the type system rather than a sentinel like `u64::MAX`.
- **Half-open / 0-indexed vs 1-indexed ranges** — `TakeNum(3)` with `total=12` returns `Some(3 - 1) = Some(2)`, because line 3 is at index 2. The `[start, total)` convention covers `start..=total-1` in 0-indexed = `(start+1)..=total` in 1-indexed.
- **`read_until(b'\n', &mut Vec<u8>)` vs `lines()`** — `read_until` reads up to *and including* the next `\n`, appending into a reusable `Vec<u8>`. Unlike `lines()`, it does **not** strip the trailing newline — essential for faithfully reproducing the original byte stream.
- **`String::from_utf8_lossy` and split codepoints** — substitutes `U+FFFD` for invalid UTF-8 sequences. Not what GNU `tail -c` does (GNU copies raw bytes); the test suite masks the divergence by reading expected files lossy too. Byte-faithful output would use `io::stdout().write_all(&buf)`.
- **`once_cell::sync::OnceCell<T>` vs `std::sync::LazyLock<T>` (Rust 1.80+)** — both compile-once lazy statics; `LazyLock` is the std drop-in. The deeper move is to drop the regex entirely when the grammar is trivial (`^[+-]?\d+$` = two char checks + one `parse`).
- **`clap` derive: `conflicts_with`, `required = true`, `default_value`** — `-n` and `-c` are mutually exclusive via `conflicts_with("lines")`; `files` is `required = true` positional; `lines` has `default_value = "10"`.
- **`str::chars().next()` + slicing vs `str::split_first`** — for inspecting a leading `+`/`-` sign. `chars().next()` is allocation-free; `&val[1..]` slices the rest. (On non-ASCII input this would panic on a char boundary — `tail`'s numeric grammar is ASCII-only, so it's safe here.)

---

## What problem does `tail` solve?

Log inspection, mostly. *"Show me the last 100 lines of the nginx access log."* *"Show me everything after line 500 of this CSV."* *"Show me the last 4 KB of this binary."* The streaming-follow variant (`tail -f`) is a different program — this chapter handles the **static selection only**.

## Requirements

1. Accept one or more file paths (required positional).
2. Accept `-n LINES` (default 10) or `-c BYTES` (mutually exclusive via `conflicts_with`).
3. Parse the count value with the `+`/`-` sign convention:
   - Bare number `N` == `-N` (count from end)
   - `+N` == count from start
   - `+0` is a special sentinel (print entire file from beginning)
4. Print the selected portion of each file to stdout.
5. When printing multiple files, prefix each section with `==> filename <==`.
6. Support `-q` / `--quiet` to suppress headers.
7. On file-open error, print to stderr and continue.

`get_start_index` returns `Option<u64>`: `None` means "no output," `Some(offset)` means "start here."

## The offset system — `TakeValue` enum and sign convention

The sign convention is the single most confusing part of `tail`. The `TakeValue` enum encodes it cleanly:

```rust
#[derive(Debug, PartialEq)]
enum TakeValue {
    PlusZero,          // +0: special sentinel, print everything from start
    TakeNum(i64),      // positive = from start, negative = from end
}
```

### The parsing rules

A bare integer (no sign) is treated as if it were negative:

```
parse_num("10")   ->  TakeNum(-10)   from end
parse_num("+10")  ->  TakeNum(10)    from start
parse_num("-10")  ->  TakeNum(-10)   from end (same as bare)
parse_num("+0")   ->  PlusZero       entire file
parse_num("0")    ->  TakeNum(0)     zero items (no output)
```

A simple parser that handles this without a regex:

```rust
fn parse_num(val: &str) -> Result<TakeValue> {
    // <- Modernization: no regex. Inspect the first char, then parse the rest.
    // The book used once_cell::OnceCell<Regex> + Regex::new(r"^([+-])?(\d+)$").
    // std::sync::LazyLock would replace once_cell, but split_first + parse
    // eliminates the regex entirely.
    let (sign, digits) = match val.chars().next() {
        Some('+') => ("+", &val[1..]),
        Some('-') => ("-", &val[1..]),
        _         => ("-", val),       // <- bare number defaults to '-'
    };
    let num: i64 = digits.parse()
        .map_err(|_| anyhow::anyhow!("{val}"))?;

    // wrapping_neg handles i64::MIN safely: -i64::MIN would overflow,
    // wrapping_neg wraps to i64::MIN instead of panicking.
    let signed = if sign == "-" { num.wrapping_neg() } else { num };

    if sign == "+" && num == 0 {
        Ok(PlusZero)
    } else {
        Ok(TakeNum(signed))
    }
}
```

The key design decision: **a bare `N` is negated at parse time**, so `get_start_index` always works with the convention *positive = from start, negative = from end, zero = no output*. This keeps the index math consistent and concentrates the sign-handling in one place.

### `wrapping_neg` and `i64::MIN`

`-i64::MIN` would be `i64::MAX + 1`, which overflows. In debug builds this panics; in release it wraps. `i64::wrapping_neg` always wraps by design, so `i64::MIN.wrapping_neg()` returns `i64::MIN`. This is the only safe way to negate at the type's boundary. The test `parse_num(i64::MAX.to_string())` expects `TakeNum(i64::MIN + 1)` — confirming the wrapping behavior.

### `once_cell` vs `std::sync::LazyLock`

The book uses `once_cell::sync::OnceCell<Regex>` to compile the regex once and reuse it. As of Rust 1.80, `std::sync::LazyLock` is in std and is a drop-in replacement:

```rust
// Book:
static NUM_RE: OnceCell<Regex> = OnceCell::new();
let re = NUM_RE.get_or_init(|| Regex::new(r"^([+-])?(\d+)$").unwrap());

// Modern (Rust 1.80+):
use std::sync::LazyLock;
static NUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([+-])?(\d+)$").unwrap());
let _ = &*NUM_RE;          // initialized on first access
```

But the deeper move is to **drop the regex entirely** — the grammar `^[+-]?\d+$` is two character checks and one `parse`. `chars().next() + str::parse` does the job with no dependency and no lazy static at all.

## Counting vs seeking — two-pass for lines, one-pass for bytes

`tail` must decide where to start reading. The strategy differs fundamentally between line mode and byte mode.

```
// Byte mode (one pass):
//   seek(start) -> read rest -> O(1) + O(N)
//
// Line mode (two passes):
//   count_lines() -> skip(start) -> print -> 2 × O(N)
```

### Byte mode (`-c`) — `Seek` is viable

Byte offsets are known to the filesystem. We can use `Seek` to jump directly:

```rust
fn print_bytes<T: Read + Seek>(
    mut file: T,
    num_bytes: &TakeValue,
    total_bytes: i64,
) -> Result<()> {
    if let Some(start) = get_start_index(num_bytes, total_bytes) {
        file.seek(SeekFrom::Start(start))?;        // jump straight there
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        if !buf.is_empty() {
            print!("{}", String::from_utf8_lossy(&buf));
        }
    }
    Ok(())
}
```

`Seek` is a trait for "random access within a stream." `SeekFrom::Start(u64)` jumps forward from the beginning; `SeekFrom::End(i64)` jumps relative to the end. The trait bound `Read + Seek` says *"I need both — read bytes and reposition the cursor."*

### Line mode (`-n`) — must count

Lines have no fixed width. You cannot seek to "line N" without reading every byte before it. So:

1. First pass: count every line in the file (`count_lines_bytes`).
2. Math: compute the start index from total and desired offset (`get_start_index`).
3. Second pass: read from byte 0, skip lines before start, print the rest.

```rust
fn print_lines(
    mut file: impl BufRead,
    num_lines: &TakeValue,
    total_lines: i64,
) -> Result<()> {
    if let Some(start) = get_start_index(num_lines, total_lines) {
        let mut line_num = 0;
        let mut buf = Vec::new();
        loop {
            let bytes_read = file.read_until(b'\n', &mut buf)?;
            if bytes_read == 0 { break; }
            if line_num >= start {
                print!("{}", String::from_utf8_lossy(&buf));
            }
            line_num += 1;
            buf.clear();
        }
    }
    Ok(())
}
```

`read_until(b'\n', ...)` is `BufRead`'s byte-delimited reader — it reads up to and including the next `\n`, appending into a reusable `Vec<u8>`. Unlike `lines()`, it does **not** strip the trailing newline, so we can faithfully reproduce the original byte stream.

### Why not `SeekFrom::End` for byte mode?

You could skip the two-pass counting entirely and use `SeekFrom::End(-n)` for "last n bytes." The book keeps the unified `count_lines_bytes + get_start_index` infrastructure for both modes, which reduces code duplication and makes edge-case handling consistent. A production `tail -c` could use `SeekFrom::End` directly, but the unified approach is a reasonable tradeoff for a teaching tool.

### The two-pass cost and `tail -f`

The two-pass line mode means `O(N)` disk reads even when you want only the last 10 lines. For huge log files this is acceptable; for growing files (live logs) you need `tail -f`, which uses `inotify`/`kqueue` to react to appends — a different program with a different architecture (event loop, not batch).

## `get_start_index` — the heart of the utility

This function converts a `TakeValue` + total count into a start offset (or `None` == no output). It is the single most important function in the chapter:

```rust
fn get_start_index(take_val: &TakeValue, total: i64) -> Option<u64> {
    match take_val {
        PlusZero => if total > 0 { Some(0) } else { None },
        TakeNum(num) => {
            if num == &0 || total == 0 || num > &total {
                None
            } else {
                let start = if num < &0 { total + num } else { num - 1 };
                Some(if start < 0 { 0 } else { start as u64 })
            }
        }
    }
}
```

### Edge-case table

| Input                  | total | Result      | Rationale                                  |
|------------------------|-------|-------------|--------------------------------------------|
| `PlusZero` (`+0`)      | 0     | `None`      | empty file, nothing to print               |
| `PlusZero` (`+0`)      | > 0   | `Some(0)`   | print everything from start                |
| `TakeNum(0)`           | any   | `None`      | zero items requested                       |
| `TakeNum(n>0)`         | 0     | `None`      | empty file                                 |
| `TakeNum(n>0)`, `n > total` | any | `None`    | start past end, nothing to print           |
| `TakeNum(n>0)`, `n ≤ total` | any | `Some(n - 1)` | 0-indexed: line N = index N-1          |
| `TakeNum(-n)`, `n ≤ total`  | any | `Some(total - n)` | from end                            |
| `TakeNum(-n)`, `n > total`  | any | `Some(0)`  | clamp to start, print everything           |

### Walk-through for a 12-line file (`twelve.txt`)

| Argument   | `TakeValue`   | total | start     | Output                       |
|------------|---------------|-------|-----------|------------------------------|
| (default)  | `TakeNum(-10)`| 12    | `Some(2)` | lines 3–12 (last 10)         |
| `-n 3`     | `TakeNum(-3)` | 12    | `Some(9)` | lines 10–12 (last 3)         |
| `-n +3`    | `TakeNum(3)`  | 12    | `Some(2)` | lines 3–12 (from line 3)     |
| `-n +0`    | `PlusZero`    | 12    | `Some(0)` | all 12 lines                 |
| `-n 0`     | `TakeNum(0)`  | 12    | `None`    | no output                    |
| `-n 200`   | `TakeNum(-200)`| 12   | `Some(0)` | clamped — all 12 lines       |

#### The 0-indexed conversion

`TakeNum(3)` with `total=12` returns `Some(3 - 1) = Some(2)` — line 3 is at index 2. This is the half-open range convention: `[start, total)` covers lines `start..=total-1` in 0-indexed terms, which is lines `(start+1)..=total` in 1-indexed terms.

```
total = 12
TakeNum(3):  start = 3 - 1 = 2   ->  print lines at index 2,3,4,...,11  (1-indexed: 3..12)
TakeNum(-3): start = 12 + -3 = 9 ->  print lines at index 9,10,11       (1-indexed: 10..12)
```

## Multi-file headers

When multiple files are given, `tail` prints a header before each:

```
==> tests/inputs/twelve.txt <==
three
four
...
eleven
twelve
==> tests/inputs/one.txt <==
Öne line, four wordś.
```

Note the leading `\n` separator before every header except the first. The book's code:

```rust
if !args.quiet && num_files > 1 {
    println!(
        "{}==> {} <==",
        if file_num > 0 { "\n" } else { "" },
        filename.display(),
    );
}
```

`file_num > 0` introduces the separator before every header except the first — a clean way to handle the "delimiter between items, not after" pattern.

## `String::from_utf8_lossy` and multibyte boundaries

`one.txt` contains `Öne line, four wordś.` — `Ö` is 2 bytes (`0xC3 0x96`), `ś` is 2 bytes (`0xC5 0x9B`). A `tail -c 3` of this 24-byte file selects the last 3 bytes `0x9B '.' '\n'` — which splits the `ś` codepoint.

`String::from_utf8_lossy` substitutes `U+FFFD` for the orphan continuation byte, yielding `.`. The test golden file reflects this.

This is the same lossy-UTF-8 behavior seen in ch8's `cut -b`. It is **not** what GNU `tail -c` does (GNU copies raw bytes), but the test suite masks the divergence by reading expected files via `from_utf8_lossy` too. For a byte-faithful `tail -c`, write raw bytes via `io::stdout().write_all(&buf)` and skip the UTF-8 round-trip.

## Full implementation

```rust
use crate::TakeValue::*;
use anyhow::{anyhow, Result};
use clap::Parser;
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
#[command(author, version, about)]
/// Rust version of `tail`
struct Args {
    /// Input file(s)
    #[arg(required = true)]
    files: Vec<PathBuf>,                                       // <- PathBuf, not String

    /// Number of lines
    #[arg(value_name = "LINES", short('n'), long, default_value = "10")]
    lines: String,

    /// Number of bytes
    #[arg(value_name = "BYTES", short('c'), long, conflicts_with("lines"))]
    bytes: Option<String>,

    /// Suppress headers
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Debug, PartialEq)]
enum TakeValue {
    PlusZero,            // +0: print everything from start
    TakeNum(i64),        // positive = from start, negative = from end
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    let lines = parse_num(&args.lines)
        .map_err(|e| anyhow!("illegal line count -- {e}"))?;
    let bytes = args.bytes.as_deref()
        .map(parse_num)
        .transpose()
        .map_err(|e| anyhow!("illegal byte count -- {e}"))?;

    let num_files = args.files.len();
    for (file_num, filename) in args.files.iter().enumerate() {
        match File::open(filename) {
            Err(err) => eprintln!("{}: {err}", filename.display()),
            Ok(file) => {
                if !args.quiet && num_files > 1 {
                    println!(
                        "{}==> {} <==",
                        if file_num > 0 { "\n" } else { "" },
                        filename.display(),
                    );
                }
                let (total_lines, total_bytes) = count_lines_bytes(filename)?;
                let file = BufReader::new(file);
                if let Some(num_bytes) = &bytes {
                    print_bytes(file, num_bytes, total_bytes)?;
                } else {
                    print_lines(file, &lines, total_lines)?;
                }
            }
        }
    }
    Ok(())
}

// <- Modernization: no regex, no once_cell/LazyLock. Just inspect the first
//    char and parse the rest. The book used once_cell::sync::OnceCell<Regex>.
//    As of Rust 1.80 you'd use std::sync::LazyLock, but chars().next() + parse
//    eliminates the regex entirely.
fn parse_num(val: &str) -> Result<TakeValue> {
    let (sign, digits) = match val.chars().next() {
        Some('+') => ("+", &val[1..]),
        Some('-') => ("-", &val[1..]),
        _         => ("-", val),       // bare number defaults to '-'
    };
    let num: i64 = digits.parse().map_err(|_| anyhow::anyhow!("{val}"))?;

    // wrapping_neg: i64::MIN.wrapping_neg() == i64::MIN (no panic at boundary)
    let signed = if sign == "-" { num.wrapping_neg() } else { num };

    if sign == "+" && num == 0 {
        Ok(PlusZero)
    } else {
        Ok(TakeNum(signed))
    }
}

fn count_lines_bytes(filename: &Path) -> Result<(i64, i64)> {
    let mut file = BufReader::new(File::open(filename)?);
    let mut num_lines = 0;
    let mut num_bytes = 0;
    let mut buf = Vec::new();
    loop {
        let n = file.read_until(b'\n', &mut buf)?;
        if n == 0 { break; }
        num_lines += 1;
        num_bytes += n as i64;
        buf.clear();
    }
    Ok((num_lines, num_bytes))
}

// Byte mode: one-pass seek + read. Seek is viable because byte offsets
// are known to the filesystem.
fn print_bytes<T: Read + Seek>(
    mut file: T,
    num_bytes: &TakeValue,
    total_bytes: i64,
) -> Result<()> {
    if let Some(start) = get_start_index(num_bytes, total_bytes) {
        file.seek(SeekFrom::Start(start))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        if !buf.is_empty() {
            print!("{}", String::from_utf8_lossy(&buf));
        }
    }
    Ok(())
}

// Line mode: two-pass. First pass counts lines (above), second pass
// skips to the start index and prints. Lines have no fixed width,
// so seeking to "line N" is impossible without reading everything before it.
fn print_lines(
    mut file: impl BufRead,
    num_lines: &TakeValue,
    total_lines: i64,
) -> Result<()> {
    if let Some(start) = get_start_index(num_lines, total_lines) {
        let mut line_num = 0;
        let mut buf = Vec::new();
        loop {
            let n = file.read_until(b'\n', &mut buf)?;
            if n == 0 { break; }
            if line_num >= start {
                print!("{}", String::from_utf8_lossy(&buf));
            }
            line_num += 1;
            buf.clear();
        }
    }
    Ok(())
}

fn get_start_index(take_val: &TakeValue, total: i64) -> Option<u64> {
    match take_val {
        PlusZero => if total > 0 { Some(0) } else { None },
        TakeNum(num) => {
            if num == &0 || total == 0 || num > &total {
                None
            } else {
                let start = if num < &0 { total + num } else { num - 1 };
                Some(if start < 0 { 0 } else { start as u64 })
            }
        }
    }
}
```

## Key takeaways

- **`tail` is a two-axis selector: unit (lines/bytes) × direction (from end / from start).** The sign convention — bare `N` means from end, `+N` means from start — is the opposite of programmer intuition and the core source of confusion.
- **`TakeValue` enum encodes the three cases** (`PlusZero`, `TakeNum(positive)`, `TakeNum(negative)`) in a type, concentrating sign-handling in the parser so `get_start_index` works with a uniform convention.
- **`wrapping_neg` is the only safe negation at `i64::MIN`** — `-i64::MIN` would overflow; `wrapping_neg` wraps to `i64::MIN` by design. Use it whenever you negate arbitrary `i64` input.
- **`once_cell::OnceCell` → `std::sync::LazyLock` (Rust 1.80+)** is a drop-in std replacement. The deeper move is to drop the regex entirely when the grammar is `^[+-]?\d+$` — `chars().next() + str::parse` handles it with no dependency and no lazy static.
- **Byte mode can `Seek` directly** because byte offsets are filesystem-known. Line mode must count first (two-pass) because lines have no fixed width. `Read + Seek` vs `BufRead` trait bounds express this distinction at the type level.
- **`get_start_index` returns `Option<u64>`** to unify "no output" (`None`) with "start here" (`Some(offset)`). The edge-case table — empty file, zero requested, start past end, clamp-to-start — is the heart of the utility.
- **`read_until(b'\n', ...)` preserves trailing newlines**, unlike `lines()`. This is essential for faithfully reproducing the original byte stream in byte mode.
- **`String::from_utf8_lossy` diverges from GNU `tail -c` on split codepoints**, emitting `U+FFFD` instead of raw bytes. The test suite masks this; a byte-faithful impl would `io::stdout().write_all(&buf)`.
