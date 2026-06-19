# Chapter 7: `findr` — Recursive Filesystem Traversal, Filtering & Stream-Oriented Discovery

`find` is one of the purest expressions of the Unix philosophy: walk a tree, emit records, filter them. It teaches filesystem traversal, recursive graph exploration, stream processing, predicate composition, lazy filtering pipelines, error isolation, metadata inspection, and — when you squint — query execution.

---

## Prereading

Before this chapter, make sure the following are comfortable. Each shows up implicitly in the design below.

- **Rust iterator adapters & laziness** — `map`, `filter`, `filter_map`, `take`, `for_each`. An iterator chain does no work until consumed; each stage yields one item at a time. `filter_map` is the canonical "map-with-fallible-skip" combinator and is central to this chapter.
- **`Option` / `Result` combinators** — `Result::ok`, `is_match`, `?`, pattern matching on `Err`. Knowing when a function returns `Result<DirEntry, walkdir::Error>` and *why* (caller decides policy on per-entry failure).
- **`Path` vs `PathBuf`, and `OsStr`** — `Path` is borrowed, `PathBuf` is owned/growable; `file_name()` returns `Option<&OsStr>`. `to_string_lossy()` converts `OsStr` to a UTF-8-ish `Cow<str>` for regex matching. The CLI parses positional roots as `Vec<PathBuf>`, never `Vec<String>`.
- **`std::fs::read_dir` semantics** — the std primitive `read_dir` yields `Result<DirEntry>` (the std one — *different type* from `walkdir::DirEntry`), is non-recursive, lazy, and surfaces per-entry errors via `Result`. `walkdir` wraps this pattern with recursion + cycle protection.
- **Filesystem metadata & types** — `FileType` (`is_file`, `is_dir`, `is_symlink`), the distinction between a path and the inode it points at, and why `lstat` (don't follow) vs `stat` (follow) matters for symlinks.
- **clap v4 derive macros** — `Parser`, `ValueEnum`, `arg(...)`, `value_parser(...)`, `ArgAction`, `num_args`, `default_value`. The derive → attribute style, not the builder API.
- **`regex::Regex::new` is a compile step** — `Regex::new(&str) -> Result<Regex, regex::Error>` parses + builds the NFA/DFA once; `is_match` is a cheap subsequent call. Naïve code that recompiles inside a loop is a real performance bug.
- **Stream vs collect** — why `collect::<Vec<_>>().join("\n")` is O(n) memory and delayed, while `println!` inside the loop is O(1) memory and pipe-friendly.

---

## What problem does `find` solve?

Picture a project tree:

```
project/
├── src/
│   ├── main.rs
│   └── lib.rs
├── tests/
│   └── integration.rs
├── Cargo.toml
└── README.md
```

Without `find`, every "where are the files that match X?" question needs custom code. With `find`, the same question becomes a one-liner:

```sh
find . -name '*.rs'        # every Rust file
find . -type f             # every regular file (no dirs, no symlinks)
find . -type d             # every directory
find . -type f -name '*.rs' # files AND named *.rs (AND-combined)
```

The deeper insight: **`find` is a query engine**. The filesystem is the database, the walker is the storage engine, the `-name`/`-type` flags are the predicate, and the iterator pipeline is the execution plan. Once you see it this way, the same mental model transfers directly to SQL `WHERE` clauses, Kafka filters, MapReduce, web crawlers, observability pipelines, and log processors.

## The real Unix mental model

A filesystem *looks* like a tree:

```
root
├── dir
│   ├── file
│   └── file
└── dir
```

But directories form a tree, while **symbolic links form graph edges**:

```
dir1/loop -> ../dir1   # a symlink pointing at an ancestor
```

If you blindly follow links during traversal, you loop forever. Experienced systems programmers immediately ask three questions about any filesystem walk: **cycles? permissions? broken links?** The `walkdir` crate answers all three by default — its `follow_links` setting defaults to `false`, so symlinks are yielded as themselves (not traversed into). That is why `-type l` returns the symlink path rather than recursing into its target.

## Why `walkdir`?

You could hand-roll `fs::read_dir()` + manual recursion, but then you must handle the recursion stack, symlink cycles, per-entry errors, the iterator interface, and depth control. `walkdir` already solved all of these. Rule of thumb in systems code: **build business logic, reuse infrastructure.**

`WalkDir::new(".")` returns a struct whose `into_iter()` yields `Result<DirEntry, walkdir::Error>` — i.e. an `Iterator<Item = Result<DirEntry>>`. The filesystem is treated as a stream: discover one entry, process one entry, discover the next. Nothing is buffered ahead of the loop.

```
┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌────────┐
│ WalkDir  │ ->│ err map  │ ->│ type flt │ ->│ name flt │ ->│ stdout │
│ (tree)   │   │(skip+    │   │ -t f/d/l │   │ -n regex │   │        │
│          │   │ report)  │   │          │   │          │   │        │
└──────────┘   └──────────┘   └──────────┘   └──────────┘   └────────┘
```

This pipeline is exactly how Spark, Flink, and Kafka Streams structure record flows — operators chained, never "collect everything first."

## CLI design

```sh
findr                          # default: everything under "."
findr src tests                # multiple roots
findr -n '.*\.rs$'             # name filter (regex)
findr -t f                     # type filter
findr -t f -n '.*\.rs$'        # both, AND-combined
findr -t l f                   # one -t carrying multiple values
```

The author models this with three fields:

- `paths: Vec<PathBuf>` — variadic positional, default `.` (current dir).
- `names: Vec<Regex>` — repeated `-n`/`--name`, each value parsed into a *compiled* `Regex`.
- `entry_types: Vec<EntryType>` — repeated `-t`/`--type`, each parsed into a typed enum.

### Why regex instead of glob?

GNU `find -name` uses shell globs (`*.rs`). The book chooses `Regex` to demonstrate compiled search programs and richer matching (`-n '.*[.]csv'` matches `main.csv`, `data.csv`, etc.). The regex is matched against the **basename** (`entry.file_name()`), not the full path — exactly like GNU `find -name`.

### Compile once, match many times

The key clap mechanic is `value_parser(Regex::new)`:

```rust
#[arg(
    short('n'),
    long("name"),
    value_name = "NAME",
    value_parser(Regex::new),   // parse + compile at CLI parse time
    action(ArgAction::Append),  // repeated -n accumulates into Vec
    num_args(0..),              // one occurrence may carry several values
)]
names: Vec<Regex>,
```

`value_parser(Regex::new)` hands clap a function `fn(&str) -> Result<Regex, regex::Error>`. clap calls it once per `--name` occurrence during parsing, so by the time `run()` executes, every `Regex` is already compiled and validated. A bad regex like `--name '*.csv'` fails at parse time with `error: invalid value '*.csv'` — *before any file is touched*. This is the same "compile once, match many" principle behind SQL query plans, routing tables, and firewall rules.

### `ArgAction::Append` + `num_args(0..)`

Two clap attributes combine to enable both invocation styles:

- `ArgAction::Append` — repeated occurrences accumulate (`-n a -n b` → `vec![a, b]`).
- `num_args(0..)` — a single occurrence may carry zero-or-more values (`-t l f` → `vec![l, f]` in one shot).

Drop `num_args(0..)` and `-t l f` would parse `l` for `-t` and treat `f` as a positional path — **silently wrong**. Both are load-bearing.

## The `EntryType` abstraction

The author uses a small enum to model filesystem concepts rather than raw strings:

```rust
#[derive(Debug, Clone, ValueEnum, Eq, PartialEq)]
enum EntryType {
    #[value(name = "d")] Dir,
    #[value(name = "f")] File,
    #[value(name = "l")] Link,
}
```

`#[derive(ValueEnum)]` + per-variant `#[value(name = ...)]` is the modern clap v4 form. The book's source instead wrote a 13-line manual `impl ValueEnum` with `value_variants()` and `to_possible_value()` — functionally identical, but the derive is what an experienced Rust programmer reaches for today. It removes an entire category of bugs (typos in string matching), gives you free `--help` text, and makes the value space statically checkable.

## Predicates and composition

A predicate is just `item -> bool`. The entry either passes or fails. `findr` builds two:

```rust
fn matches_type(entry: &DirEntry, types: &[EntryType]) -> bool {
    types.is_empty()                              // no constraint = universal predicate
        || types.iter().any(|t| match t {
            EntryType::Link => entry.file_type().is_symlink(),
            EntryType::Dir  => entry.file_type().is_dir(),
            EntryType::File => entry.file_type().is_file(),
        })
}

fn matches_name(entry: &DirEntry, names: &[Regex]) -> bool {
    names.is_empty()
        || names.iter().any(|re| re.is_match(&entry.file_name().to_string_lossy()))
}
```

Two design patterns live here:

1. **"Empty means match everything."** When `types` or `names` is empty, the predicate returns `true` for all entries. This is the same convention SQL query engines use: *no `WHERE` clause = universal predicate*. It means caller code never has to special-case "no filters supplied."
2. **AND-composition via chained `.filter()` calls.** `.filter(matches_type).filter(matches_name)` is logically `type_match AND name_match`. Each filter stage removes records that fail — identical to `WHERE type='file' AND name LIKE '%.rs'`. *The iterator chain is the query plan.*

Extracting the predicates as free functions (instead of inline closures borrowing `&args`) also makes them **unit-testable in isolation** — a real improvement over the book's closures-inside-`run` style.

## Error isolation — report, don't abort

Walking a filesystem can fail at *any* entry: permission denied, broken symlink, file deleted between discovery and `stat`, I/O error. `WalkDir` yields `Result<DirEntry>`, not `DirEntry`, precisely so the caller can decide. Two natural patterns exist:

```rust
// (A) Silent discard — drops the entry AND the error:
.filter_map(Result::ok)

// (B) Report-and-continue — logs to stderr, drops the entry:
.filter_map(|e| match e {
    Err(e)   => { eprintln!("{e}"); None }
    Ok(entry) => Some(entry),
})
```

The author uses **(B)**. This matters: an integration test creates a `chmod 000` directory and asserts that stderr contains `"cant-touch-this: Permission denied"` *and* that the other 17 entries still appear on stdout. Pattern (A) would silently swallow the permission error, the test would fail, and a user running `findr /` over a system with restricted dirs would get mysteriously incomplete output.

**Report-and-continue is the right default for any scan over untrusted or partially-accessible hierarchies** — the same philosophy large-scale ETL and log processors use: *skip the bad record, log it, keep going.*

## The full pipeline

```rust
for path in &args.paths {
    for entry in WalkDir::new(path)
        .into_iter()
        .filter_map(|e| match e {                 // report-and-continue
            Err(e)    => { eprintln!("{e}"); None }
            Ok(entry) => Some(entry),
        })
        .filter(|e| matches_type(e, &args.entry_types))
        .filter(|e| matches_name(e, &args.names))
    {
        println!("{}", entry.path().display());   // stream straight to stdout
    }
}
```

Notice what is **not** here: `.collect::<Vec<_>>()` followed by `entries.join("\n")`. The book's source materializes the entire result set into a `Vec<String>` and then joins — `O(matches)` memory. The streaming form above is `O(1)` memory: each match is printed and dropped immediately. For a scan over millions of files the difference is hundreds of MB vs a handful of bytes. Streaming also means partial output appears in real time, so a downstream `| head` can short-circuit early.

## Full implementation

```rust
use anyhow::Result;
use clap::{ArgAction, Parser, ValueEnum};
use regex::Regex;
use std::path::PathBuf;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Parser)]
#[command(author, version, about)]
/// Rust version of `find`
struct Args {
    /// Search path(s)
    #[arg(value_name = "PATH", default_value = ".")]
    paths: Vec<PathBuf>,                          // <- PathBuf, not String

    /// Names to match (regex, tested against basename)
    #[arg(
        short('n'), long("name"), value_name = "NAME",
        value_parser(Regex::new),                 // compile once at parse time
        action(ArgAction::Append),                // -n a -n b -> vec![a, b]
        num_args(0..)                             // one -n may carry several values
    )]
    names: Vec<Regex>,

    /// Entry types to match
    #[arg(
        short('t'), long("type"), value_name = "TYPE",
        value_parser(clap::value_parser!(EntryType)),
        action(ArgAction::Append),
        num_args(0..)
    )]
    entry_types: Vec<EntryType>,
}

// <- Derived ValueEnum replaces the book's 13-line manual impl.
// `#[value(name = ...)]` maps each variant to its short CLI string.
#[derive(Debug, Clone, ValueEnum, Eq, PartialEq)]
enum EntryType {
    #[value(name = "d")] Dir,
    #[value(name = "f")] File,
    #[value(name = "l")] Link,
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    for path in &args.paths {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|e| match e {              // <- report-and-continue
                Err(e)    => { eprintln!("{e}"); None }
                Ok(entry) => Some(entry),
            })
            .filter(|e| matches_type(e, &args.entry_types))
            .filter(|e| matches_name(e, &args.names))
        {
            println!("{}", entry.path().display()); // <- stream, don't collect
        }
    }
    Ok(())
}

// <- Predicates extracted as free functions so they're unit-testable.
// "Empty means match everything" — no constraint = universal predicate.
fn matches_type(entry: &DirEntry, types: &[EntryType]) -> bool {
    types.is_empty()
        || types.iter().any(|t| match t {
            EntryType::Link => entry.file_type().is_symlink(),
            EntryType::Dir  => entry.file_type().is_dir(),
            EntryType::File => entry.file_type().is_file(),
        })
}

fn matches_name(entry: &DirEntry, names: &[Regex]) -> bool {
    names.is_empty()
        || names.iter().any(|re| re.is_match(&entry.file_name().to_string_lossy()))
}
```

## Key takeaways

- **`find` is a query engine disguised as a Unix utility.** Filesystem = database, `WalkDir` = storage engine, filters = predicate, iterator pipeline = execution plan. The same shape recurs in databases, search engines, log processors, web crawlers, and distributed data systems.
- **`walkdir`'s default `follow_links = false` is what makes the "filesystems are graphs, not trees" concern tractable** — symlinks are yielded as themselves, never traversed into, so cycles cannot form.
- **`value_parser(Regex::new)` compiles the regex once, at parse time.** Bad patterns fail before any file is touched. "Compile once, match many" is a systems-programming universal.
- **`ArgAction::Append` + `num_args(0..)` together** enable both `-t f -t d` (repeated) and `-t f d` (multi-value). Drop either and one style breaks silently.
- **Derived `ValueEnum` + `#[value(name = ...)]`** replaces the manual `impl ValueEnum` with three attribute lines. Same behavior, less code, fewer bugs.
- **"Empty means match everything"** is the universal-predicate convention — no constraint = pass all. Caller code never special-cases the "no filters" branch.
- **Report-and-continue, not silent discard.** `filter_map(|e| match e { Err => { eprintln!(...); None }, Ok => Some(...) })` preserves observability. `filter_map(Result::ok)` silently erases errors and breaks the permission-denied test contract.
- **Stream, do not collect.** `println!` inside the loop is `O(1)` memory and pipes-friendly. `.collect::<Vec<_>>().join("\n")` is `O(matches)` memory and delays all output until the end.
