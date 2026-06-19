# Chapter 12: `fortuner` — Random Quote Selection, File Discovery, and Seeded Randomness

The Unix `fortune` program picks a random epigram, quote, or joke from a collection of text files and prints it. It is used for login messages, email signatures, screen savers, and — relevant to us — **deterministic test fixtures**.

The challenge for a Rust reimplementation is threefold:

1. **File discovery** — fortune files live in directories alongside `.dat` index files (compiled by the C `strfile` command) that must be excluded.
2. **Parsing a delimited format** — quotes are separated by `%` on a line by itself.
3. **Deterministic randomness** — a seeded RNG lets tests reproduce the same fortune every run.

> **Note on `rand`:** This crate pins `rand = "0.10"`. The book was written against `rand = "0.8.5"`, and `rand` has had two rounds of breaking changes since (0.9 and 0.10). The code below is the modernized 0.10 form — see [rand 0.8 → 0.10 migration](#rand-08--010-migration) for the full delta.

---

## Prereading

Before this chapter, make sure the following are comfortable. Each shows up implicitly in the design below.

- **`rand` 0.10 trait hierarchy** — `Rng` is the user-facing, dyn-compatible trait (infallible RNG); `RngCore` is the deprecated low-level trait (use `Rng` instead); `RngExt` adds `random` / `random_range` / `sample_iter` / `sample`; `TryRng` is the fallible variant. `StdRng` implements `TryRng<Error = Infallible>` and thus `Rng` via blanket impl. `Box<dyn Rng>` is the canonical way to unify different RNG concrete types behind one trait object.
- **`SeedableRng::seed_from_u64(u64)`** — constructs a deterministic RNG from a `u64` seed. `StdRng::seed_from_u64(1)` always produces the same sequence. This is a *test fixture*, not a security feature (the docs explicitly warn `seed_from_u64` is not for security-sensitive use).
- **`rand::seq::IndexedRandom::choose`** — the 0.10 home of single-element slice sampling. `slice.choose(&mut rng) -> Option<&T>`. (In 0.8 this lived on `SliceRandom`; 0.10 split sampling into `IndexedRandom` and shuffling into `SliceRandom`.)
- **`Box<dyn Trait>` and trait objects** — when two match arms would return different concrete types (`StdRng` vs `ThreadRng`), a `Box<dyn Rng>` erases the concrete type so both arms unify. Cost: one heap alloc + dynamic dispatch. The alternative is a generic `fn pick<R: Rng>(rng: &mut R)` that monomorphizes.
- **`std::ffi::OsStr`** — the OS-native string slice. `Path::extension()` returns `Option<&OsStr>`; comparing against `OsStr::new("dat")` avoids UTF-8 validation cost and handles non-Unicode filenames correctly. Prefer it over `str`/`String` for extension matching.
- **`Vec<String>::join("\n")` vs `push_str` + `trim`** — `join` gives exact control: `n` lines produce `n - 1` newlines, no trailing whitespace to trim. This is the canonical delimiter-format parser pattern.
- **`Option::as_deref()`** — converts `Option<String>` to `Option<&str>` for cheap comparison without cloning. The 2024 idiom for "have we seen this value before?" is `prev.as_deref() != Some(&current)`.
- **`walkdir` + `filter_map(Result::ok)`** — silently drops traversal errors. Acceptable for trusted directories; for adversarial inputs use `filter_map` + `eprintln!` (same shape as ch7's `findr`).
- **`BufRead::lines` + `map_while(Result::ok)`** — line iteration that silently halts at the first I/O error. Recurs across chapters (ch9 `grepr`, ch11 `tailr`); a deliberate "good enough" choice, not a best practice.
- **clap v4: `required = true` on `Vec<T>` positionals** — a `Vec<T>` positional defaults to `num_args = 0..` (optional, possibly empty). To force "at least one FILE," you need `required = true`. Without it, no args silently yields `vec![]` instead of a usage error.
- **`RegexBuilder` vs `Regex::new`** — when `case_insensitive` is a runtime flag from `-i`, use `RegexBuilder` (same rationale as ch9 `grepr`).

---

## The fortune file format — `%` delimited

Fortune files are plain text where each entry is separated by a line containing exactly `%`:

```
Q. What do you call a head of lettuce in a shirt and tie?
A. Collared greens.
%
Q: Why did the gardener quit his job?
A: His celery wasn't high enough.
%
Q: What do you call a deer wearing an eye patch?
A: A bad idea (bad-eye deer).
%
```

Rules:

- The delimiter is `%` and nothing else — no leading/trailing whitespace.
- Entries can be one line or many lines.
- The delimiter marks the boundary between entries; it is *not* part of any entry's text.
- A trailing `%` after the last entry is harmless (just terminates an empty buffer that gets skipped).

```
┌──────────────────────────────────────────────────────────┐
│ line = "Q. What do you call a head of lettuce..."        │
│ line != "%"  ->  buffer.push(line)                       │
│ line = "A. Collared greens."                             │
│ line != "%"  ->  buffer.push(line)                       │
│ line = "%"                                                │
│ line == "%"  ->  buffer not empty?                       │
│                  fortunes.push(buffer.join("\n"))         │
│                  buffer.clear()                          │
│ ...                                                       │
└──────────────────────────────────────────────────────────┘
```

## The buffer-and-join pattern

The canonical way to parse this format line-by-line:

```rust
for line in BufReader::new(file).lines().map_while(Result::ok) {
    if line == "%" {
        if !buffer.is_empty() {
            fortunes.push(Fortune {
                source: basename.clone(),
                text: buffer.join("\n"),          // <- join, not push_str + trim
            });
            buffer.clear();
        }
    } else {
        buffer.push(line);
    }
}
```

Why `buffer.join("\n")` instead of accumulating into a `String` with `push_str`?

- `BufRead::lines()` strips the trailing `\n` from each line.
- `push_str` with an explicit newline would add a trailing `\n` that must then be trimmed.
- `Vec::join("\n")` gives exact control: `n` lines produce `n - 1` newline characters, no trimming needed.

```
buffer = ["line1", "line2", "line3"]
buffer.join("\n") -> "line1\nline2\nline3"     // no trailing \n
```

## File discovery — `walkdir` + `OsStr` `.dat` filtering

```rust
fn find_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let dat = OsStr::new("dat");
    let mut files = vec![];
    for path in paths {
        match fs::metadata(path) {
            Err(e) => bail!("{}: {e}", path.display()),
            Ok(_) => files.extend(
                WalkDir::new(path)
                    .into_iter()
                    .filter_map(Result::ok)                       // silently drop unreadable entries
                    .filter(|e| e.file_type().is_file()
                        && e.path().extension() == Some(dat))
                    .map(|e| e.path().to_path_buf()),
            ),
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}
```

Two design notes:

- **`OsStr` comparison** — `Path::extension()` returns `Option<&OsStr>`. Constructing `OsStr::new("dat")` lets you compare OS-native byte representations without any UTF-8 conversion cost, and it handles non-Unicode filenames correctly. `String`/`str` comparison would force a lossy conversion that could mismatch.
- **`sort + dedup`** guarantees deterministic ordering when the same file appears via overlapping source paths. The `pick_fortune` selection is random, but `find_files` ordering **must** be deterministic so a given seed always produces the same fortune.

Note: `filter_map(Result::ok)` here silently drops traversal errors — same tradeoff as ch9's `WalkDir::flatten()`. For a fortune file indexer over trusted directories this is fine; for adversarial inputs use `filter_map` + `eprintln!`.

## Random selection — `Box<dyn Rng>` for seeded vs unseeded

```rust
fn pick_fortune(fortunes: &[Fortune], seed: Option<u64>) -> Option<String> {
    if fortunes.is_empty() {
        return None;
    }
    let mut rng: Box<dyn Rng> = match seed {
        Some(val) => Box::new(StdRng::seed_from_u64(val)),   // deterministic
        None      => Box::new(rand::rng()),                   // entropy from OS
    };
    fortunes.choose(&mut rng).map(|f| f.text.clone())
}
```

### Why `Box<dyn Rng>`?

`IndexedRandom::choose` is generic over `R: Rng`. Both `StdRng` (seeded) and `ThreadRng` (entropy) implement `Rng`, but they are *different concrete types*. Without the `Box<dyn Rng>` indirection, the two match arms would return different concrete types and the function would not compile — match arms must all return the same type.

The trait object erases the concrete type so both arms unify. The cost is one heap allocation (`Box::new`) and dynamic dispatch on each `rng` call — both negligible for a single `choose` call.

### Alternative — generic helper

A generic version avoids the box:

```rust
fn pick_with<R: Rng>(fortunes: &[Fortune], rng: &mut R) -> Option<&Fortune> {
    fortunes.choose(rng)
}

// caller:
let mut rng = StdRng::seed_from_u64(seed);
let f = pick_with(&fortunes, &mut rng);
```

This monomorphizes per concrete RNG and avoids the heap allocation. The tradeoff: the caller has to branch on `seed` and instantiate the right RNG before calling, which spreads the seeded/unseeded logic across the call site. The boxed form keeps the decision in one place. **For a CLI that picks one fortune per invocation, the boxed form is clearer; for a tight loop the generic form wins.**

### Seeded determinism is a test fixture

```rust
#[test]
fn test_pick_fortune() {
    assert_eq!(
        pick_fortune(&fortunes, Some(1)).unwrap(),
        "Neckties strangle clear thinking."
    );
}
```

This test passes every time because seed `1` deterministically produces the same index. That is the entire point of `--seed`: **reproducibility**. Production fortunes are entropy-sourced; test fortunes are seeded. The same code path, different RNG construction.

## `rand` 0.8 → 0.10 migration

The book pins `rand = "0.8.5"`. This crate uses `rand = "0.10"`, which has two rounds of breaking changes (0.9 and 0.10) on top of 0.8. The full delta:

| Concern                       | rand 0.8                                  | rand 0.10 (this crate)                              |
|-------------------------------|-------------------------------------------|-----------------------------------------------------|
| Thread RNG                    | `rand::thread_rng()`                      | `rand::rng()` (old name removed)                    |
| Single-element slice sampling | `rand::seq::SliceRandom::choose`          | `rand::seq::IndexedRandom::choose` (trait split)   |
| Shuffling                     | `rand::seq::SliceRandom::shuffle`         | `rand::seq::SliceRandom::shuffle` (unchanged)       |
| User-facing RNG trait         | `RngCore` (low-level) + `Rng` (extension) | `Rng` (dyn-compatible, user-facing) + `RngExt`      |
| Trait object for RNG          | `Box<dyn RngCore>`                        | `Box<dyn Rng>` (`RngCore` deprecated since 0.10)    |
| `StdRng` algorithm            | ChaCha8                                   | ChaCha12                                            |
| Distributions module          | `rand::distributions::*`                  | `rand::distr::*`                                    |
| `gen()` / `gen_range()`       | `Rng::gen`, `Rng::gen_range`              | `RngExt::random`, `RngExt::random_range`            |
| `SeedableRng::seed_from_u64`  | unchanged                                 | unchanged                                           |

The seeded branch (`StdRng::seed_from_u64`) is the one piece that survived untouched. Everything else — the import paths, the trait object type, the sampling trait — moved.

## Pattern-matching mode

When the user supplies `-m` / `--pattern`, the program switches from "pick one" to "print all matches":

```rust
match pattern {
    Some(pattern) => {
        let mut prev_source = None;
        for fortune in fortunes.iter().filter(|f| pattern.is_match(&f.text)) {
            // Source header (e.g. "(jokes)") printed to STDERR on first match per source
            if prev_source.as_deref() != Some(&fortune.source) {
                eprintln!("({})\n%", fortune.source);
                prev_source = Some(fortune.source.clone());
            }
            println!("{}\n%", fortune.text);
        }
    }
    None => {
        println!("{}", pick_fortune(&fortunes, args.seed)
            .unwrap_or_else(|| "No fortunes found".to_string()));
    }
}
```

Key details:

- **`RegexBuilder` (not `Regex::new`)** is used because `.case_insensitive(args.insensitive)` is a runtime flag — same rationale as ch9's `grepr`.
- The **source header is printed to stderr**, matching the C `fortune -m` convention — stdout stays clean for piping the fortunes themselves.
- Fortunes are separated by `\n%\n`, recreating valid fortune-file format on stdout (you could pipe the output back into another `fortuner`).
- **`prev_source.as_deref() != Some(&fortune.source)`** — the idiomatic 2024 form of "is this a new source?" `as_deref()` converts `Option<String>` to `Option<&str>` so you can compare against `Some(&String)` without cloning. (The book's `prev_source.as_ref().map_or(true, |s| s != &fortune.source)` is older style and reverses the polarity awkwardly.)

## `required(true)` for variadic positionals

```rust
#[arg(required = true, value_name = "FILE")]
sources: Vec<PathBuf>,
```

In clap v4, a `Vec<T>` positional defaults to `num_args = 0..` — i.e. *not required*. To force "at least one FILE," you need `required = true` (or the function-call form `required(true)`). Without it, `fortuner` with no args would silently default to an empty `Vec` and print "No fortunes found" instead of a clap usage error. The `dies_not_enough_args` test verifies this — it expects `the following required arguments were not provided: <FILE>...`.

## Full implementation

```rust
use anyhow::{anyhow, bail, Result};
use clap::Parser;
use rand::seq::IndexedRandom;                 // <- rand 0.10: choose moved here (was SliceRandom)
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};                 // <- Rng for trait object; SeedableRng for seed_from_u64
                                              //    (RngCore is deprecated since 0.10; use Rng)
use regex::RegexBuilder;
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::PathBuf,
};
use walkdir::WalkDir;

#[derive(Debug, Parser)]
#[command(author, version, about)]
/// Rust version of `fortune`
struct Args {
    /// Input files or directories
    #[arg(required = true, value_name = "FILE")]
    sources: Vec<PathBuf>,                    // <- required = true (modern form); PathBuf, not String

    /// Pattern
    #[arg(short('m'), long)]
    pattern: Option<String>,

    /// Case-insensitive pattern matching
    #[arg(short, long)]
    insensitive: bool,

    /// Random seed
    #[arg(short, long)]
    seed: Option<u64>,                        // <- clap infers u64 parser; no value_parser! needed
}

#[derive(Debug)]
struct Fortune {
    source: String,
    text: String,
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    let pattern = args.pattern.as_deref()
        .map(|p| RegexBuilder::new(p)
            .case_insensitive(args.insensitive)
            .build()
            .map_err(|_| anyhow!(r#"Invalid --pattern "{p}""#)))
        .transpose()?;
    let files = find_files(&args.sources)?;
    let fortunes = read_fortunes(&files)?;

    match pattern {
        Some(pattern) => {
            let mut prev_source = None;
            for f in fortunes.iter().filter(|f| pattern.is_match(&f.text)) {
                // <- as_deref() != Some(&..) is the 2024 idiom for "new source?"
                if prev_source.as_deref() != Some(&f.source) {
                    eprintln!("({})\n%", f.source);
                    prev_source = Some(f.source.clone());
                }
                println!("{}\n%", f.text);
            }
        }
        None => {
            println!("{}", pick_fortune(&fortunes, args.seed)
                .unwrap_or_else(|| "No fortunes found".to_string()));
        }
    }
    Ok(())
}

fn find_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let dat = OsStr::new("dat");
    let mut files = vec![];
    for path in paths {
        match fs::metadata(path) {
            Err(e) => bail!("{}: {e}", path.display()),
            Ok(_) => files.extend(
                WalkDir::new(path)
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().is_file()
                        && e.path().extension() == Some(dat))
                    .map(|e| e.path().to_path_buf()),
            ),
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

// <- Buffer-and-join: accumulate lines into Vec<String>, join with "\n"
//    so N lines produce N-1 newlines with no trailing whitespace to trim.
fn read_fortunes(paths: &[PathBuf]) -> Result<Vec<Fortune>> {
    let mut fortunes = vec![];
    let mut buffer = vec![];
    for path in paths {
        let basename = path.file_name()
            .ok_or_else(|| anyhow!("no filename for {}", path.display()))?
            .to_string_lossy()
            .into_owned();
        let file = File::open(path)
            .map_err(|e| anyhow!("{}: {e}", path.display()))?;
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if line == "%" {
                if !buffer.is_empty() {
                    fortunes.push(Fortune {
                        source: basename.clone(),
                        text: buffer.join("\n"),
                    });
                    buffer.clear();
                }
            } else {
                buffer.push(line);
            }
        }
    }
    Ok(fortunes)
}

// <- Box<dyn Rng> unifies StdRng (seeded) and ThreadRng (entropy)
//    under one type so both match arms return the same thing.
//    Cost: one heap alloc + dynamic dispatch — negligible for one pick.
//    (rand 0.10: Rng replaces the deprecated RngCore trait object.)
fn pick_fortune(fortunes: &[Fortune], seed: Option<u64>) -> Option<String> {
    if fortunes.is_empty() {
        return None;
    }
    let mut rng: Box<dyn Rng> = match seed {
        Some(val) => Box::new(StdRng::seed_from_u64(val)),
        None      => Box::new(rand::rng()),     // <- rand 0.10: rng() replaces thread_rng()
    };
    fortunes.choose(&mut rng).map(|f| f.text.clone())   // <- IndexedRandom::choose (was SliceRandom)
}
```

## Key takeaways

- **The buffer-and-join pattern** (`Vec<String>` + `.join("\n")`) is the canonical way to parse delimiter-separated text formats. `N` lines produce `N-1` newlines exactly — no trailing whitespace to trim, no `push_str` + `trim_end` dance.
- **`OsStr` is the correct type for extension filtering.** `Path::extension()` returns `Option<&OsStr>`; comparing against `OsStr::new("dat")` avoids UTF-8 validation cost and handles non-Unicode filenames.
- **`Box<dyn Rng>` unifies different RNG types** (`StdRng`, `ThreadRng`) under one trait object so both match arms return the same type. The cost — one heap alloc + dynamic dispatch — is negligible for one pick. For tight loops, a generic `fn pick<R: Rng>(rng: &mut R)` monomorphizes and avoids the box. (In `rand` 0.10, use `Rng`, not the deprecated `RngCore`, as the trait object.)
- **`IndexedRandom::choose` is the 0.10 home of slice sampling** — `SliceRandom` lost `choose` in 0.10 and now only has `shuffle`/`partial_shuffle`. If you import `SliceRandom` expecting `.choose()`, it will not compile.
- **Seeded determinism (`StdRng::seed_from_u64`) is a test fixture, not a UX feature:** seed `1` always produces the same index, so tests can assert exact output. Production picks are entropy-sourced; the same code path, different RNG construction.
- **`prev_source.as_deref() != Some(&f.source)`** is the 2024 idiom for "have we seen this source before?" `as_deref()` converts `Option<String>` to `Option<&str>` for cheap comparison without cloning.
- **`required = true` on a `Vec<T>` positional is mandatory** if you want "at least one." Without it, clap defaults `Vec` positionals to `num_args = 0..` (optional, possibly empty), and the program silently gets `vec![]` instead of a usage error.
- **`sort + dedup` after `find_files`** makes ordering deterministic so a given seed always produces the same fortune, regardless of filesystem traversal order.
- **`map_while(Result::ok)` silently halts on I/O error.** Acceptable for trusted local files; for production use `filter_map` + logging to surface read failures.
