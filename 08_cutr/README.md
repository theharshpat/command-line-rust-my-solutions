# Chapter 8: `cutr` — Column, Field, and Byte Extraction from Streams

`cut` extracts slices of each line of a text stream — by field, by byte, or by character. It is one of the most-used data-munging tools in Unix, sitting at the foundation of every CSV/TSV/log-parsing pipeline.

```sh
cat data.tsv      | cut -f1,3            # field columns 1 and 3 (tab-delimited)
curl api.com/export.csv | cut -d, -f2-5  # CSV fields 2 through 5
ps aux            | cut -c1-80           # first 80 characters of each line
cat /etc/passwd   | cut -d: -f1,3-5      # username and uid/gid/gecos
```

---

## Prereading

Before this chapter, make sure the following are comfortable. Each shows up implicitly in the design below.

- **`Range<usize>` and half-open slicing** — `start..end` is exclusive of `end`; `slice[a..b]` panics on out-of-bounds but `slice.get(a..b)` returns `Option`. This chapter uses `slice.get(i)` everywhere so out-of-bounds positions drop silently rather than panic.
- **Iterator adapters, especially `flat_map` + `filter_map`** — extraction is literally `positions.iter().flat_map(|r| r.filter_map(|i| ...))`. Knowing the difference between `map` (1:1) and `filter_map` (1:0-or-1) is load-bearing here.
- **`str::split_once` vs `str::split`** — `split_once(char)` returns `Option<(&str, &str)>` with zero allocations; `split` returns an iterator that you usually have to collect. The chapter's range parser leans on `split_once('-')` instead of a regex.
- **`NonZeroUsize` and niche optimization** — `NonZeroUsize` is `usize` with a type-level proof of non-zeroness. `str::parse::<NonZeroUsize>()` returns `Err` for `"0"`, so the zero-rejection rule is enforced by the parser, not by an `if` you forgot to write. `Option<NonZeroUsize>` is the same size as `usize` (niche optimization) — a bonus, not the primary reason.
- **`String::from_utf8_lossy`** — substitutes `U+FFFD` (REPLACEMENT CHARACTER) for any invalid UTF-8 sequence. Free correctness for "mostly text" data, but lossy at the byte level — relevant when comparing to GNU `cut -b`.
- **`Box<dyn BufRead>` and trait objects** — one `open()` function returns a `Box<dyn BufRead>` so stdin and `File` go through the same `lines()` API. Recap dynamic dispatch + `?`-based error conversion if rusty.
- **clap v4 derive: `Args`, `#[group]`, `#[command(flatten)]`** — the canonical pattern for "exactly one of these mutually-exclusive options." Validation moves into the parser; error messages are uniform.
- **The `csv` crate: `ReaderBuilder`, `StringRecord`, `has_headers`, `WriterBuilder`** — RFC 4180 quoting on the way in *and* on the way out. The default `has_headers(true)` would silently eat the first row.
- **1-indexed vs 0-indexed** — `cut` is 1-indexed on the command line; Rust slices are 0-indexed. The conversion happens once in the parser and never again.

---

## What problem does `cut` solve?

Given structured or semi-structured text, extract specific columns, fields, byte ranges, or character positions from every line. That is the foundational operation of data reshaping in Unix.

## Three extraction modes

`cut` offers three fundamentally different selection strategies. Each mode reflects a different mental model of "position" in a line:

```sh
cut -f2     # field-level: CSV record indexing (split on delimiter)
cut -b1-3   # byte-level:  raw Vec<u8> slicing
cut -c1-3   # char-level:  Unicode code point indexing
```

The three are **mutually exclusive** — you cannot ask for both fields and bytes in one invocation. They share the same range syntax (`1,3,5` or `1-5` or `1,3-5,9`), so the parser is common; only the extraction algorithm differs.

## Requirements

| #  | Requirement           | Detail                                                      |
|----|-----------------------|-------------------------------------------------------------|
| 1  | Accept files and stdin | `-` means stdin; process multiple files                     |
| 2  | Support `-f` (fields)  | Delimiter-separated column extraction                       |
| 3  | Support `-b` (bytes)   | Raw byte index extraction                                   |
| 4  | Support `-c` (chars)   | Unicode character extraction                                |
| 5  | Range syntax          | `1,3,5`; `1-5`; `1,3-5,9`                                   |
| 6  | Exactly one mode      | `-f`, `-b`, `-c` mutually exclusive                          |
| 7  | Single-byte delimiter | `-d,` must be exactly one byte                              |
| 8  | Error recovery        | One bad file does not kill the whole pipeline               |

## Why the delimiter matters — and the CSV quoting trap

The default field delimiter is tab (TSV). That is no accident: tab-delimited data was the standard interchange format before CSV won. When you parse CSV with a naive single-byte split you immediately face three problems:

- **Fields containing the delimiter** (`a,"b,c",d` — the second field has a comma)
- **Fields containing newlines** (multiline CSV cells)
- **Escaped quotes** (`""` for a literal `"`)

Standard Unix `cut -d,` handles none of these correctly — it splits on every comma. So `echo 'a,"b,c",d' | cut -d, -f2` outputs `"b`, which is wrong. The chapter's Rust version improves on this by using the `csv` crate for field mode, which parses RFC 4180 quoting properly and yields `b,c` for that same input.

There is a tradeoff: **correctness vs speed.** The `csv` crate is slower than a byte split. But for any data with quoting, the speed cost is worth it.

## Forcing exactly one mode

The first design decision: how to enforce "exactly one of `-f`, `-b`, `-c`"?

### Option A — separate args + manual validation

```rust
#[derive(Debug, Parser)]
struct Args {
    #[arg(short, long)] fields: Option<String>,
    #[arg(short, long)] bytes:  Option<String>,
    #[arg(short, long)] chars:  Option<String>,
}
```

Then in `run`, manually check that exactly one is `Some`. This works, but it pushes validation into runtime code and forces you to hand-write the error message.

### Option B — `clap::#[group]` (what the author chose)

```rust
#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]
struct ArgsExtract {
    #[arg(short, long, value_name = "FIELDS")] fields: Option<String>,
    #[arg(short, long, value_name = "BYTES")]  bytes:  Option<String>,
    #[arg(short, long, value_name = "CHARS")]  chars:  Option<String>,
}

#[derive(Debug, Parser)]
struct Args {
    #[arg(default_value = "-")] files: Vec<PathBuf>,                       // <- PathBuf, not String
    #[arg(short, long, value_name = "DELIMITER", default_value = "\t")]
    delimiter: String,
    #[command(flatten)] extract: ArgsExtract,                              // inline the group into Args
}
```

`#[group(required = true, multiple = false)]` tells clap: *"at least one must appear, and no more than one."* Validation moves into the parser; error messages come out consistent with every other clap error (`the following required arguments were not provided: <--fields|--bytes|--chars>`).

`#[command(flatten)]` inlines the sub-struct into `Args` so the user sees a flat CLI surface.

## The `Extract` enum and position storage

Once parsed, the three possibilities collapse into one enum carrying the parsed position list:

```rust
type PositionList = Vec<Range<usize>>;

#[derive(Debug)]
enum Extract {
    Fields(PositionList),
    Bytes(PositionList),
    Chars(PositionList),
}
```

`PositionList = Vec<Range<usize>>` — every position is a half-open `Range<usize>` (`start..end`, exclusive end). User 1-indexed `N` becomes `N-1..N`; range `N-M` becomes `N-1..M`. The 1→0 conversion happens **once in the parser** and never again at extraction time. Out-of-bounds positions are silently dropped via `slice.get(i)` returning `None` — matching GNU `cut`'s "missing position → empty" behavior.

The enum carries the same inner type in every variant, so the enum only chooses the algorithm; the position slice parameterizes it. Clean separation.

## Parsing range syntax

The grammar is: comma-separated segments, each either a single integer or `N-M`:

```
"1,3,5"     -> [0..1, 2..3, 4..5]
"1-5"       -> [0..5]
"1,3-5,9"   -> [0..1, 2..5, 8..9]
```

```
              "1,3-5,9"
                  │
                  │  split(',')
                  ▼
            ["1", "3-5", "9"]
              │     │     │
              │     │     │  split_once('-') -> None
              │     │     ▼
              │     │     parse_index("9") -> 8
              │     │     -> 8..9
              │     │
              │     │     split_once('-') -> Some(("3","5"))
              │     ▼
              │     parse_index("3")=2
              │     parse_index("5")=4
              │     2 < 4? yes
              │     -> 2..5
              │
              │     split_once('-') -> None
              ▼
              parse_index("1") -> 0
              -> 0..1

   collect() -> [0..1, 2..5, 8..9]
```

Two rules worth knowing:

1. **Zero is illegal.** `cut` is 1-indexed; `0` and `0-1` both error with `illegal list value: "0"`.
2. **Range start must be less than range end.** `1-1` and `2-1` both error.

The book uses a `Regex` for the `N-M` segment. That is overkill — the grammar is `N` or `N-M`, full stop. `str::split_once('-')` is allocation-free, has no dependency, and is easier to read:

```rust
fn parse_pos(range: &str) -> Result<PositionList> {
    range
        .split(',')
        .map(|seg| match seg.split_once('-') {
            None => {
                let n = parse_index(seg)?;
                Ok(n..n + 1)
            }
            Some((a, b)) => {
                let n1 = parse_index(a)?;
                let n2 = parse_index(b)?;
                if n1 >= n2 {
                    bail!("First number in range ({}) must be lower than second ({})",
                        n1 + 1, n2 + 1);
                }
                Ok(n1..n2 + 1)
            }
        })
        .collect()
}

// Parse a 1-indexed position, returning the 0-indexed usize.
// Rejects 0 and leading '+'.
fn parse_index(input: &str) -> Result<usize> {
    if input.starts_with('+') {
        bail!(r#"illegal list value: "{input}""#);
    }
    Ok(usize::from(input.parse::<NonZeroUsize>()?) - 1)
}
```

### Why `NonZeroUsize`?

`NonZeroUsize` is `usize` with a type-level proof of non-zeroness. `str::parse::<NonZeroUsize>()` returns `Err` for `"0"`, so the zero-rejection rule is enforced *by the parser*, not by an `if` we forgot to write. After parsing, `usize::from(n) - 1` is the 0-indexed value. The "non-zero" guarantee is upheld at construction time; everything after can rely on it.

One important nuance: the guarantee is a **runtime check** (inside `parse`), not a compile-time one. The type makes the proof available to the compiler (e.g. for niche optimizations where `Option<NonZeroUsize>` is the same size as `usize`), but `parse` itself runs at runtime. Do not claim "enforced at compile time" — it is enforced by the type system once constructed, and construction is a runtime operation.

## Differences from GNU `cut` worth knowing

The chapter's impl diverges from GNU `cut` in two ways the notes should not gloss over:

- **No open-ended ranges.** GNU `cut` accepts `1-` (from 1 to end of line) and `-5` (from start to 5). This impl's grammar rejects both. If you depend on those forms, this `cutr` will not be a drop-in replacement.
- **No deduplication or sorting.** GNU `cut -f1,1` outputs field 1 once, and `cut -f3,1` outputs in ascending order `1,3`. This impl preserves user order and duplicates: `cutr -c 1,1` prints the first character twice on each line. That is why the `repeated_value` test's expected output is `AA / ÉÉ / SS / JJ` rather than `A / É / S / J`.

## The three extraction algorithms

The whole chapter collapses into three small functions, one per mode:

```
   file ──► open() ──► Box<dyn BufRead>
                 │
       ┌─────────┼───────────────────────────────────┐
       │         │                                   │
    Fields    Bytes                              Chars
       │         │                                   │
  csv::Reader  line.as_bytes()              line.chars().collect()
       │         │                                   │
  StringRecord  Vec<u8> indexing               Vec<char> indexing
       │         │                                   │
 extract_fields extract_bytes                 extract_chars
       │         │                                   │
  csv::Writer  String::from_utf8_lossy            String
       │         │                                   │
       └─────────┴───────────────────────────────────┘
                 │
               stdout
```

```rust
fn extract_fields<'a>(record: &'a StringRecord, pos: &[Range<usize>]) -> Vec<&'a str> {
    pos.iter()
        .flat_map(|r| r.filter_map(|i| record.get(i)))
        .collect()
}

fn extract_bytes(line: &str, pos: &[Range<usize>]) -> String {
    let bytes = line.as_bytes();
    let sel: Vec<u8> = pos.iter()
        .flat_map(|r| r.filter_map(|i| bytes.get(i)).copied())
        .collect();
    String::from_utf8_lossy(&sel).into_owned()
}

fn extract_chars(line: &str, pos: &[Range<usize>]) -> String {
    let chars: Vec<char> = line.chars().collect();
    pos.iter().flat_map(|r| r.filter_map(|i| chars.get(i))).collect()
}
```

### Byte mode and the lossy-UTF-8 trap

`extract_bytes` uses `String::from_utf8_lossy`, which substitutes `U+FFFD` for any invalid UTF-8 sequence. This is *not* what GNU `cut -b` does. GNU `cut -b` is byte-oriented and copies raw bytes verbatim — if byte 8 of a multibyte character is selected, that raw continuation byte goes to stdout. The Rust impl round-trips through UTF-8 validation and may emit replacement characters instead.

The test suite masks this divergence: the golden files are generated by running GNU `cut` (raw bytes), and the Rust test reads them back via `String::from_utf8_lossy`, so both sides collapse to the same `String` and the byte-equality check passes. But at the raw-byte level GNU emits `0xA9` while `cutr` emits `0xEF 0xBF 0xBD`.

**Do not claim "this matches GNU `cut` behavior."** It matches at the *lossy-String* level, not at the byte level. A production-faithful `cut -b` would write raw bytes via `io::stdout().write_all(&sel)` and skip the UTF-8 round-trip entirely.

### Why `has_headers(false)` is load-bearing in field mode

The `csv` crate defaults to treating the first record as a header row and skipping it. For `cut`, every line is a uniform record — there is no header. So:

```rust
let mut reader = ReaderBuilder::new()
    .delimiter(delimiter)
    .has_headers(false)            // <- without this, the first row disappears
    .from_reader(file);
```

Drop `has_headers(false)` and `cut -f1` on a CSV with a `title,year,director` header would silently drop the title line. The book's expected-output files include the header row, so this flag is what makes the test pass.

### One subtlety of the CSV-aware writer

The author also writes output via `csv::WriterBuilder`, not via plain `println!`. That means the writer applies RFC 4180 quoting on the way out. So if a selected field's content contains the delimiter (e.g. `b,c` from `a,"b,c",d`), the writer re-quotes it as `"b,c"` on stdout. GNU `cut` would emit the bare `b,c`. This is a second behavioral divergence from GNU `cut` — reading is more correct (good), but writing is also more quoting-aware (different). Neither direction is tested by the integration suite.

## Full implementation

```rust
use anyhow::{anyhow, bail, Result};
use clap::Parser;
use csv::{ReaderBuilder, StringRecord, WriterBuilder};
use std::{
    fs::File,
    io::{self, BufRead, BufReader},
    num::NonZeroUsize,
    ops::Range,
    path::PathBuf,
};

#[derive(Debug, Parser)]
#[command(author, version, about)]
/// Rust version of `cut`
struct Args {
    /// Input file(s)
    #[arg(default_value = "-")]
    files: Vec<PathBuf>,                       // <- PathBuf, not String

    /// Field delimiter
    #[arg(short, long, value_name = "DELIMITER", default_value = "\t")]
    delimiter: String,

    #[command(flatten)]
    extract: ArgsExtract,
}

#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]     // <- exactly one of -f/-b/-c
struct ArgsExtract {
    #[arg(short, long, value_name = "FIELDS")] fields: Option<String>,
    #[arg(short, long, value_name = "BYTES")]  bytes:  Option<String>,
    #[arg(short, long, value_name = "CHARS")]  chars:  Option<String>,
}

type PositionList = Vec<Range<usize>>;

#[derive(Debug)]
enum Extract {
    Fields(PositionList),
    Bytes(PositionList),
    Chars(PositionList),
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    // Validate delimiter is exactly one byte.
    let delim_bytes = args.delimiter.as_bytes();
    if delim_bytes.len() != 1 {
        bail!(r#"--delim "{}" must be a single byte"#, args.delimiter);
    }
    let delimiter: u8 = *delim_bytes.first().unwrap();

    // Parse the one supplied mode into an Extract. The clap #[group] guarantee
    // makes the unreachable! branch genuinely unreachable.
    let extract = if let Some(f) = args.extract.fields.as_deref().map(parse_pos).transpose()? {
        Extract::Fields(f)
    } else if let Some(b) = args.extract.bytes.as_deref().map(parse_pos).transpose()? {
        Extract::Bytes(b)
    } else if let Some(c) = args.extract.chars.as_deref().map(parse_pos).transpose()? {
        Extract::Chars(c)
    } else {
        unreachable!("clap #[group(required, multiple = false)] guarantees one is set")
    };

    for filename in &args.files {
        match open(filename) {
            Err(err) => eprintln!("{}: {err}", filename.display()),    // report, continue
            Ok(file) => match &extract {
                Extract::Fields(pos) => {
                    let mut rdr = ReaderBuilder::new()
                        .delimiter(delimiter)
                        .has_headers(false)                             // <- load-bearing
                        .from_reader(file);
                    let mut wtr = WriterBuilder::new()
                        .delimiter(delimiter)
                        .from_writer(io::stdout());
                    for record in rdr.records() {
                        wtr.write_record(extract_fields(&record?, pos))?;
                    }
                }
                Extract::Bytes(pos) => {
                    for line in file.lines() {
                        println!("{}", extract_bytes(&line?, pos));
                    }
                }
                Extract::Chars(pos) => {
                    for line in file.lines() {
                        println!("{}", extract_chars(&line?, pos));
                    }
                }
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

// <- No regex. split_once('-') handles the N or N-M grammar directly.
fn parse_pos(range: &str) -> Result<PositionList> {
    range
        .split(',')
        .map(|seg| match seg.split_once('-') {
            None => {
                let n = parse_index(seg)?;
                Ok(n..n + 1)
            }
            Some((a, b)) => {
                let n1 = parse_index(a)?;
                let n2 = parse_index(b)?;
                if n1 >= n2 {
                    bail!("First number in range ({}) must be lower than second ({})",
                        n1 + 1, n2 + 1);
                }
                Ok(n1..n2 + 1)
            }
        })
        .collect()
}

fn parse_index(input: &str) -> Result<usize> {
    if input.starts_with('+') {
        bail!(r#"illegal list value: "{input}""#);
    }
    Ok(usize::from(input.parse::<NonZeroUsize>()?) - 1)
}

fn extract_fields<'a>(record: &'a StringRecord, pos: &[Range<usize>]) -> Vec<&'a str> {
    pos.iter()
        .flat_map(|r| r.filter_map(|i| record.get(i)))
        .collect()
}

fn extract_bytes(line: &str, pos: &[Range<usize>]) -> String {
    let bytes = line.as_bytes();
    let sel: Vec<u8> = pos.iter()
        .flat_map(|r| r.filter_map(|i| bytes.get(i)).copied())
        .collect();
    String::from_utf8_lossy(&sel).into_owned()
}

fn extract_chars(line: &str, pos: &[Range<usize>]) -> String {
    let chars: Vec<char> = line.chars().collect();
    pos.iter().flat_map(|r| r.filter_map(|i| chars.get(i))).collect()
}
```

## Key takeaways

- **`cut` has three extraction modes reflecting three mental models of "position":** field (delimiter-split), byte (raw `Vec<u8>` slice), char (Unicode code point). They share range syntax but use three different algorithms.
- **`#[group(required = true, multiple = false)]` + `#[command(flatten)]`** is the canonical clap v4 pattern for "exactly one of these mutually-exclusive options." Validation moves into the parser; error messages are uniform.
- **`NonZeroUsize` enforces the "no zero" rule by type, not by `if`.** The guarantee is runtime-checked at construction and statically available thereafter — niche optimization is a bonus, not the primary reason.
- **`str::split_once('-')` beats `Regex` for the `N` or `N-M` grammar.** Same behavior, no dependency, no allocation, easier to read. Reach for regex when the grammar is actually irregular.
- **Half-open `Range<usize>` with 1→0 conversion done once in the parser** keeps extraction code uniform. Out-of-bounds positions drop silently via `slice.get(i)`.
- **`csv` crate field mode is more correct than GNU `cut` for quoted input** (`a,"b,c",d` -> `b,c`), but the `csv` writer re-quotes on output, which is different from GNU `cut`'s pass-through. Both directions of divergence should be acknowledged.
- **`String::from_utf8_lossy` in byte mode is not what GNU `cut -b` does.** GNU copies raw bytes; this impl emits `U+FFFD` for split codepoints. The test suite masks the difference by reading expected files lossy too. Do not call this "matching GNU `cut`."
- **Two real divergences from GNU `cut`:** no open-ended ranges (`1-`, `-5`), and no dedup/sort of the position list (`cut -f1,1` prints field 1 twice instead of once). Worth knowing before you treat `cutr` as a drop-in.
