# Chapter 13: `calr` — Calendar Display, Date Arithmetic, and Grid Layout

`cal` prints a calendar — either a single month or a full year. The interesting systems work is not in printing text in a grid (that is just `format!`), but in **date arithmetic**: computing weekdays, month lengths, leap years, and "what day of the week was January 1, 2026?" — all without writing a calendar library by hand.

The Rust version (`calr`) supports: a single month (`-m`), a full year (positional or `-y`), today-highlighting via reverse-video ANSI, and partial month-name matching (`-m jan`, `-m fe`, `-m s` for September).

> **Note on `ansi_term`:** This crate's `Cargo.toml` still pins `ansi_term = "0.12.1"`. That crate is archived/abandoned. The code below inlines the two ANSI escape codes directly and drops the dependency — see [ansi_term is abandoned](#ansi_term-is-abandoned--inline-the-two-codes).

---

## Prereading

Before this chapter, make sure the following are comfortable. Each shows up implicitly in the design below.

- **`chrono::NaiveDate` and the `Datelike` trait** — `NaiveDate` is a timezone-less calendar date over the proleptic Gregorian calendar (Jan 1, 262144 BCE – Dec 31, 262143 CE). `Datelike` provides `.year()`, `.month()`, `.day()`, `.weekday()`. Importing `chrono::Datelike` is required for those methods to be in scope.
- **`_opt` constructors vs panicking ones** — `from_ymd_opt(y, m, d) -> Option<NaiveDate>` is the modern non-panicking form; `from_ymd` is **deprecated since chrono 0.4.23** (panics on invalid dates). Same for `pred_opt()` vs `pred()`. Always reach for the `_opt` variants; the `Option` lets the type system carry "this date might not exist."
- **`Weekday` and `number_from_sunday()`** — `weekday()` returns `Weekday` (Mon..Sun enum). `Weekday::number_from_sunday()` maps Sun=1, Mon=2, …, Sat=7 — the exact column index `cal` needs for a Sunday-first grid. (Counterpart: `number_from_monday()` for ISO/Monday-first layouts.)
- **Leap year rule** — divisible by 4, *except* centuries, *except* centuries divisible by 400. (2000 was a leap year; 1900 was not; 2024 is; 2100 will not be.) `chrono` encodes this correctly; do not re-implement.
- **`Local::now().date_naive()`** — the modern "today as `NaiveDate`" call. The older `Local::now().date()` is deprecated. Returns a `NaiveDate` (no timezone), which is what you want for calendar-grid comparison against `NaiveDate::from_ymd_opt(...)`.
- **`format!` width/alignment specifiers** — `{:^20}` center in 20, `{:>32}` right-align in 32, `{:width$}` use a runtime variable for width, `{num:>2}` right-align a number in 2. These are the layout primitives for the grid.
- **`slice::chunks(n)`** — splits a flat `Vec` into sub-slices of `n` elements (the last may be shorter). Used to chunk a flat day-list into 7-day week rows.
- **`itertools::izip!`** — zips N iterators into *flat* tuples: `izip!(a, b, c)` yields `(a0, b0, c0), (a1, b1, c1), …`. `std::iter::zip` only does 2-way and yields nested tuples `((a, b), c)`. For the 3-up year layout, `izip!` is the right tool.
- **clap v4: `value_parser!(T).range(..)`, `conflicts_with_all`, `Option<Result>.transpose()`** — `value_parser!(i32).range(1..=9999)` constrains an integer arg with a uniform error message; `conflicts_with_all(["month", "year"])` declares mutual exclusion; `args.month.map(parse_month).transpose()?` turns `Option<Result<u32>>` into `Result<Option<u32>>`, propagating parse errors while preserving the "absent" case.

---

## What problem does `cal` solve?

Quick visual reference for "what day of the week is the 17th of next month?" without reaching for a GUI. Real workflows:

- **Scheduling** — "which weeks does this month span?"
- **Release planning** — "is the 15th a weekend?"
- **Date sanity-checks** while scripting

## Requirements

- Optional positional year (1–9999, validated by clap).
- Optional `-m MONTH` accepting either a number (1–12) or a name prefix (case-insensitive, unambiguous prefix match).
- `-y` / `--year` to show the entire current year; conflicts with both `-m` and the positional year.
- Default (no args): current month, today highlighted.
- Output: 22-char-wide month grid, 8 lines tall; 3×4 layout for the full year (66 chars wide).

## The date system — why `chrono`?

You could compute weekdays with **Zeller's congruence**:

```
h = (q + floor(13(m+1)/5) + K + floor(K/4) + floor(J/4) - 2J) mod 7
```

where `q` = day, `m` = month (Jan/Feb treated as 13/14 of the prior year), `K` = year mod 100, `J` = floor(year/100). This is the textbook formula for "weekday of a given date." It is error-prone to implement: the Jan/Feb renumbering, the negative-modulus handling, the century-leap corrections all have to be exactly right, and the Gregorian cutover (1582) introduces a discontinuity.

**Do not hand-roll this.** `chrono::NaiveDate` already encodes the proleptic Gregorian calendar correctly, handles leap years (divisible by 4, except centuries, except centuries divisible by 400), and exposes `weekday()` directly. Delegating to `chrono` is the systems-programming move: *reuse the thoroughly-solved infrastructure, build your business logic on top.*

### `NaiveDate` arithmetic — month-end via "next-month-first minus one"

The trick for "how many days are in this month?":

```rust
fn last_day_in_month(year: i32, month: u32) -> NaiveDate {
    // First day of the NEXT month:
    let (y, m) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    // ...is preceded by the last day of the original month.
    NaiveDate::from_ymd_opt(y, m, 1).unwrap().pred_opt().unwrap()
}
```

```
Input: year=2024, month=2 (February, leap year)
  Step 1: next month = (2024, 3)
  Step 2: from_ymd_opt(2024, 3, 1)  -> Some(2024-03-01)
  Step 3: .pred_opt()                -> Some(2024-02-29)   <- chrono handles leap day
  Output: February 29, 2024

Input: year=2026, month=12 (December)
  Step 1: next month = (2027, 1)     <- December -> January wrap
  Step 2: from_ymd_opt(2027, 1, 1)   -> Some(2027-01-01)
  Step 3: .pred_opt()                -> Some(2026-12-31)
  Output: December 31, 2026
```

This delegates month-length (including the leap-day rule) entirely to `chrono` — no lookup table, no `if month == 2 { if is_leap { 29 } else { 28 } }`. The `pred_opt` / `from_ymd_opt` forms are the non-panicking `Option`-returning APIs; the older `pred` / `from_ymd` are **deprecated in chrono 0.4.x**. The `.unwrap()`s are safe because the inputs are constructed to be valid (year ≤ 9999, so `year+1 ≤ 10000`, well within chrono's range of ~262143).

### `Local::now().date_naive()` — the modern "today"

```rust
let today = Local::now().date_naive();
```

`date_naive()` is the modern call — the older `Local::now().date()` is deprecated. It returns a `NaiveDate` (no timezone), which is what we want for calendar-grid comparison against `NaiveDate::from_ymd_opt(year, month, day)`.

## The month grid — 22 chars × 8 lines

The fixed invariant: **every month is rendered as exactly 8 lines of exactly 22 characters.** This is what makes the 3-up year layout work — `izip!` over three months' line-vectors produces aligned 66-char rows.

```
col:   0         1         2
       0123456789012345678901
       February 2020            <- line 0: title, centered in 20 + 2 trailing spaces
       Su Mo Tu We Th Fr Sa     <- line 1: weekday names, fixed literal
                         1      <- line 2: week 1 (6 blanks + day 1, Sat)
        2  3  4  5  6  7  8      <- line 3: week 2
        9 10 11 12 13 14 15     <- line 4: week 3
       16 17 18 19 20 21 22     <- line 5: week 4
       23 24 25 26 27 28 29     <- line 6: week 5
                                <- line 7: blank padding to 8 lines
       └──────────┬──────────┘ └─┬─┘
        20-char title cell    2 trailing
                              spaces
       └───────────┬───────────┘
                 22 chars = LINE_WIDTH
```

The construction algorithm:

1. **First weekday → blank padding.** `first.weekday().number_from_sunday()` returns 1 (Sun) … 7 (Sat). The range `1..n` produces `n-1` blank `"  "` cells, so day 1 lands under its weekday column. Feb 2020 starts on Saturday (`n=7`), so 6 blanks precede the `1`.
2. **Day numbers.** `(first.day()..=last.day())` yields `1..=29` for Feb 2020. Each is `format!("{num:>2}")` — right-aligned in 2 chars, matching the 2-char blank cells. Today's day is wrapped in reverse-video ANSI.
3. **Chunk into weeks.** `days.chunks(7)` slices the flat `Vec<String>` into 7-element sub-slices. Each is `week.join(" ")` (single space between cells), then `format!("{:width$}  ", ..., width = LINE_WIDTH - 2)` pads to 20 chars and appends 2 trailing spaces → 22 total.
4. **Pad to 8 lines.** `while lines.len() < 8 { lines.push(" ".repeat(LINE_WIDTH)); }`. A 28-day February starting on Sunday fills only 4 week rows; the `while` pads the remaining rows with blank 22-char lines so `izip!` aligns.

## The year grid — 3×4 via `izip!`

```rust
let months: Vec<_> = (1..=12)
    .map(|m| format_month(year, m, false, today))
    .collect();
for (i, chunk) in months.chunks(3).enumerate() {
    if let [m1, m2, m3] = chunk {
        for (l1, l2, l3) in izip!(m1, m2, m3) {
            println!("{}{}{}", l1, l2, l3);   // 22 + 22 + 22 = 66 chars
        }
        if i < 3 { println!(); }              // blank line between quarters
    }
}
```

```
┌─────────┬─────────┬─────────┐
│  Jan    │  Feb    │  Mar    │   <- izip! yields (jan[0], feb[0], mar[0])
│  lines  │  lines  │  lines  │      concatenated into one 66-char row
│  ...    │  ...    │  ...    │
└─────────┴─────────┴─────────┘
                ▲
   blank line between quarters (i < 3)
┌─────────┬─────────┬─────────┐
│  Apr    │  May    │  Jun    │
...
```

`itertools::izip!` zips N iterators into flat tuples — `izip!(a, b, c)` yields `(a0, b0, c0), (a1, b1, c1), …`. The std alternative `a.zip(b).zip(c)` yields nested tuples `((a, b), c)` which is awkward. `std::iter::zip` (stable since 1.59) only does 2-way. For 3-way flat tuples, `izip!` is the right tool.

### The year header is right-aligned, not centered

```rust
println!("{year:>32}");
```

`>` is right-align, not `^` (center). The year appears at columns 28–32 of the 66-wide grid — roughly the Jan/Feb boundary. It is **not** centered over the grid. The visual effect is "title in the upper-right," which is what BSD/GNU `cal` both do.

## Today highlighting — ANSI reverse video

```rust
let is_today = |day: u32| {
    year == today.year() && month == today.month() && day == today.day()
};

let last = last_day_in_month(year, month);
days.extend((first.day()..=last.day()).map(|num| {
    let fmt = format!("{num:>2}");
    if is_today(num) {
        format!("\x1b[7m{fmt}\x1b[0m")
    } else {
        fmt
    }
}));
```

The reverse-video escape codes wrap the day number: `\x1b[7m 7\x1b[0m` for today = 7. `\x1b[7m` turns on reverse video; `\x1b[0m` resets all attributes.

### `ansi_term` is abandoned — inline the two codes

The book uses `ansi_term = "0.12.1"`. **`ansi_term` is deprecated/abandoned** — the upstream repo is archived and the README states it is no longer maintained. For a program that uses *one* ANSI attribute (reverse video), the entire dependency is overkill:

```rust
// <- Modernization: ansi_term is deprecated/abandoned.
// For one attribute, inline the two escape codes — zero dependencies.
if is_today(num) {
    format!("\x1b[7m{fmt:>2}\x1b[0m")
} else {
    fmt
}
```

One line, zero deps, identical output. For programs that need many attributes, the modern alternatives are **`owo-colors`** (lightweight, actively maintained) or **`nu-ansi-term`** (a maintained fork with a near-identical API to `ansi_term`). For this program, inlining wins — and lets you drop `ansi_term` from `Cargo.toml`.

## Month name parsing — number or prefix

```rust
fn parse_month(month: &str) -> Result<u32> {
    // Try numeric first.
    if let Ok(num) = month.parse::<u32>() {
        return if (1..=12).contains(&num) {
            Ok(num)
        } else {
            bail!(r#"month "{month}" not in the range 1 through 12"#)
        };
    }
    // Otherwise: case-insensitive unambiguous-prefix match on month names.
    let lower = month.to_lowercase();
    let matches: Vec<_> = MONTH_NAMES.iter().enumerate()
        .filter_map(|(i, name)| {
            if name.to_lowercase().starts_with(&lower) { Some(i as u32 + 1) } else { None }
        })
        .collect();
    if matches.len() == 1 {
        Ok(matches[0])
    } else {
        bail!(r#"Invalid month "{month}""#)
    }
}
```

The branch order matters: **numeric first, prefix second.** `"1"` is unambiguously January-as-number; `parse::<u32>()` succeeds so we never reach the prefix branch. `"ja"` is not a number, so it falls through to prefix matching, where it unambiguously matches January (no other month starts with `ja`).

The "unambiguous" rule matters:

| Input  | Matches              | Result      |
|--------|----------------------|-------------|
| `"j"`  | January, June, July  | error (3 candidates) |
| `"ju"` | June, July           | error (2 candidates) |
| `"jun"`| June                 | OK (unambiguous) |
| `"jul"`| July                 | OK (unambiguous) |
| `"ja"` | January              | OK (unambiguous) |
| `"s"`  | September            | OK (unambiguous — no other month starts with `s`) |

### Why not clap's `value_parser`?

You cannot express "number OR name-prefix" as a clap `value_parser!` — clap's typed parsers handle one type at a time. The string is kept as `Option<String>` at the clap level and the disambiguation happens in `parse_month` at runtime. This is the right call: **clap's job is "is this a string?"; the domain logic "is this a valid month spec?" belongs in your code.**

## clap patterns worth knowing

```rust
#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Args {
    /// Year (1-9999)
    #[arg(value_parser(clap::value_parser!(i32).range(1..=9999)))]
    year: Option<i32>,

    /// Month name or number (1-12)
    #[arg(short)]
    month: Option<String>,

    /// Show the whole current year
    #[arg(short('y'), long("year"), conflicts_with_all(["month", "year"]))]
    show_current_year: bool,
}
```

- **`value_parser!(i32).range(1..=9999)`** — clap parses the positional as `i32` and rejects out-of-range values with `error: invalid value '0' for '[YEAR]': 0 is not in 1..=9999`. This is the canonical way to constrain an integer arg; the error message is uniform with clap's other errors.
- **`conflicts_with_all(["month", "year"])`** — `-y` cannot be combined with `-m` or the positional year. clap emits `the argument '--year' cannot be used with '[YEAR]'` automatically.
- **`Option<Result<u32>>.transpose()?`** — `args.month.map(parse_month).transpose()?` turns `Option<Result<u32>>` into `Result<Option<u32>>`, propagating parse errors while preserving the "absent" case. A neat `Option` + `Result` interop pattern.
- **Defaulting happens at runtime, not in clap.** No `default_value` is used — because the default ("today's month/year") cannot be known at parse time. The runtime logic in `run()` handles `-y` / both-None / one-None cases explicitly.

## Full implementation

```rust
use anyhow::{bail, Result};
use chrono::{Datelike, Local, NaiveDate};
use clap::Parser;
use itertools::izip;

#[derive(Debug, Parser)]
#[command(author, version, about)]
/// Rust version of `cal`
struct Args {
    /// Year (1-9999)
    #[arg(value_parser(clap::value_parser!(i32).range(1..=9999)))]
    year: Option<i32>,

    /// Month name or number (1-12)
    #[arg(short)]
    month: Option<String>,

    /// Show the whole current year
    #[arg(short('y'), long("year"), conflicts_with_all(["month", "year"]))]
    show_current_year: bool,
}

const LINE_WIDTH: usize = 22;
const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
];

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    let today = Local::now().date_naive();
    let mut month = args.month.as_deref().map(parse_month).transpose()?;
    let mut year = args.year;

    // Runtime defaulting: -y wins; else both-None -> today; else year alone -> today.year.
    if args.show_current_year {
        month = None;
        year = Some(today.year());
    } else if month.is_none() && year.is_none() {
        month = Some(today.month());
        year = Some(today.year());
    }

    let year = year.unwrap_or(today.year());

    match month {
        Some(m) => {
            let lines = format_month(year, m, true, today);
            println!("{}", lines.join("\n"));
        }
        None => {
            // <- Right-align (not center): {year:>32} puts the year in cols 28-32.
            println!("{year:>32}");
            let months: Vec<_> = (1..=12)
                .map(|m| format_month(year, m, false, today))
                .collect();
            for (i, chunk) in months.chunks(3).enumerate() {
                if let [m1, m2, m3] = chunk {
                    for (l1, l2, l3) in izip!(m1, m2, m3) {
                        println!("{}{}{}", l1, l2, l3);   // 22+22+22 = 66 chars
                    }
                    if i < 3 { println!(); }              // blank between quarters
                }
            }
        }
    }
    Ok(())
}

fn parse_month(month: &str) -> Result<u32> {
    if let Ok(num) = month.parse::<u32>() {
        return if (1..=12).contains(&num) {
            Ok(num)
        } else {
            bail!(r#"month "{month}" not in the range 1 through 12"#)
        };
    }
    let lower = month.to_lowercase();
    let matches: Vec<_> = MONTH_NAMES.iter().enumerate()
        .filter_map(|(i, name)| {
            if name.to_lowercase().starts_with(&lower) { Some(i as u32 + 1) } else { None }
        })
        .collect();
    if matches.len() == 1 {
        Ok(matches[0])
    } else {
        bail!(r#"Invalid month "{month}""#)
    }
}

// <- Month-end via "first of next month, minus one." Delegates month-length
//    (including leap days) entirely to chrono. No lookup table.
fn last_day_in_month(year: i32, month: u32) -> NaiveDate {
    let (y, m) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(y, m, 1).unwrap().pred_opt().unwrap()
}

fn format_month(
    year: i32,
    month: u32,
    print_year: bool,
    today: NaiveDate,
) -> Vec<String> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let mut days: Vec<String> = (1..first.weekday().number_from_sunday())
        .map(|_| "  ".to_string())                          // <- 2-char blank cells for leading days
        .collect();
    let is_today = |day: u32| {
        year == today.year() && month == today.month() && day == today.day()
    };
    let last = last_day_in_month(year, month);
    days.extend((first.day()..=last.day()).map(|num| {
        let fmt = format!("{num:>2}");
        // <- Modernization: ansi_term is deprecated/abandoned.
        //    Inline the two escape codes — zero deps, identical output.
        if is_today(num) {
            format!("\x1b[7m{fmt}\x1b[0m")
        } else {
            fmt
        }
    }));

    let month_name = MONTH_NAMES[month as usize - 1];
    let mut lines = Vec::with_capacity(8);
    lines.push(format!(
        "{:^20}  ",
        if print_year { format!("{month_name} {year}") } else { month_name.to_string() }
    ));
    lines.push("Su Mo Tu We Th Fr Sa  ".to_string());
    for week in days.chunks(7) {
        lines.push(format!("{:width$}  ", week.join(" "), width = LINE_WIDTH - 2));
    }
    while lines.len() < 8 {
        lines.push(" ".repeat(LINE_WIDTH));                 // <- pad to 8 so izip! aligns
    }
    lines
}
```

## Key takeaways

- **`chrono` is the right move for date arithmetic.** Do not hand-roll Zeller's congruence — the Jan/Feb renumbering, century leaps, and Gregorian cutover are traps. `NaiveDate::weekday()` is correct by construction; `pred_opt()` for month-end is correct by construction; `Local::now().date_naive()` is the modern "today."
- **Month-end via "first of next month, minus one"** delegates month-length (including the leap-day rule) to `chrono`. No lookup table, no `if month == 2 { if is_leap { 29 } else { 28 } }`.
- **The 22-char × 8-line invariant is load-bearing.** It is what makes the 3-up year layout via `izip!` align correctly. Pad with blank lines to 8 even for short months.
- **`izip!` from `itertools` zips N iterators into flat tuples.** `std::iter::zip` only does 2-way; for 3-way flat tuples `izip!` is the right tool.
- **`{year:>32}` is right-align, not center.** The year appears in the upper-right of the 66-wide grid, matching BSD/GNU `cal`. Do not call this "centered."
- **`ansi_term` is deprecated/abandoned.** For one ANSI attribute, inline the two escape codes (`\x1b[7m` / `\x1b[0m`) — zero deps, identical output, and drop `ansi_term` from `Cargo.toml`. For many attributes, use `owo-colors` or `nu-ansi-term`.
- **`value_parser!(i32).range(1..=9999)`** is the canonical clap v4 way to constrain an integer arg. The error message is uniform with clap's other errors.
- **`conflicts_with_all`** declares that `-y` cannot combine with `-m` or the positional year. clap emits the conflict error automatically.
- **`Option<Result<T>>.transpose()?`** is a neat interop pattern: turns `Option<Result>` into `Result<Option>`, propagating errors while preserving the "absent" case.
- **Defaulting happens at runtime, not via clap `default_value`**, because "today's month/year" cannot be known at parse time. The runtime logic handles `-y` / both-None / one-None cases explicitly.
- **`parse_month` does number-then-prefix** because clap's `value_parser` handles one type at a time. Domain logic like "number OR unambiguous name prefix" belongs in your code, not in clap.
