# Chapter 14: `lsr` — Directory Listing, File Metadata, and Tabular Output

`ls` is the most-used Unix command. It lists directory contents so you can see what files exist, check their sizes, permissions, ownership, and modification times. Without `ls`, you'd be calling `readdir(2)` and `stat(2)` for every file by hand; `ls` bundles those syscalls into a polished interface with sort order, column alignment, and optional long-format metadata.

This is the **final chapter**, and it ties together themes from the whole book: filesystem traversal (ch7), metadata inspection, bitmask decoding, tabular output, and the Unix philosophy of *"one tool, one job, composed via pipes."*

> **Note on dependencies:** This crate pins `tabular = "0.2"` (column alignment) and `users = "0.11"` (UID/GID → name lookup). Both are functional but lightly maintained. Modern alternatives: **`tabled`** or **`comfy-table`** for tables, **`uzers`** (a maintained fork of `users` with the same API) for user lookup. The code below keeps the book's choices for fidelity.

---

## Prereading

Before this chapter, make sure the following are comfortable. Each shows up implicitly in the design below.

- **`std::fs::metadata` vs `std::fs::symlink_metadata`** — `metadata` *follows* symlinks (reports the target's type/size/perms); `symlink_metadata` does not (reports the link's own metadata). For `ls -l` on a symlink you need the latter, or you'd print the target's mode instead of `lrwxr-xr-x`. This is the single most common `ls`-implementation bug.
- **`std::os::unix::fs::MetadataExt`** — the Unix-specific extension trait that adds `.uid()`, `.gid()`, `.mode()`, `.nlink()`, `.mtime()`, etc. to `fs::Metadata`. Importing it is what makes this code Unix-only; Windows has ACLs, not the `rwx` model, and would need a completely different ownership/permission path.
- **Unix file mode as a bitmask** — `mode: u32` packs file type (high bits), setuid/setgid/sticky (3 bits), and 9 permission bits (3 × rwx for user/group/other). `0o755` = `rwxr-xr-x`, `0o644` = `rw-r--r--`. `mode & mask != 0` is the canonical single-bit test. This is the same bit-manipulation skill you'll need for page headers, WAL record types, and bloom filters in storage-engine code.
- **`DirEntry::metadata()` vs `path.metadata()`** — the former can return metadata the OS already cached during `read_dir`, halving syscall count. The latter issues a fresh `stat`. For large directories the difference is real.
- **`path.file_name() -> Option<&OsStr>`** — the last path component (no parent). `to_string_lossy().starts_with('.')` is the hidden-file test. `OsStr` (not `str`) handles non-UTF-8 names without lossy conversion.
- **`PathBuf` over `String` for path args** — clap's `PathBuf` value parser handles OS strings correctly; `String` would force UTF-8 and mangle non-Unicode filenames.
- **The `tabular` crate's format mini-language** — `"{:<}{:<} {:>} {:<} {:<} {:>} {:<} {:<}"`: `<` left-align, `>` right-align. The crate measures column widths from data, so narrow columns don't waste space and wide columns don't overflow. This is a real improvement over manual `format!` padding.
- **`chrono::DateTime<Local>::from(metadata.modified()?)`** — converts `SystemTime` to a local datetime for `format("%b %d %y %H:%M")` rendering.
- **Report-and-continue error isolation** — a bad path prints to stderr and the loop continues. Same pattern as ch7 `findr` and ch9 `grepr`. This is the Unix `ls` contract: `ls foo bar` succeeds for `bar` even if `foo` doesn't exist.

---

## What problem does `ls` solve?

- **Exploration** — "What's in this directory?"
- **Permissions debugging** — "Why can't I execute this script?"
- **Disk usage** — "How big are these log files?"
- **Timestamps** — "Which file was modified most recently?"

The Rust version (`lsr`) supports: one or more paths (files and/or directories), `-l` / `--long` for the seven-column metadata view, and `-a` / `--all` to show hidden files (those starting with `.`).

## Requirements

- One or more paths (default: `.`).
- `-l` / `--long`: long listing with metadata (permissions, links, owner, group, size, mtime, name).
- `-a` / `--all`: include hidden files.
- Short listing: one file per line.
- Error tolerance: bad paths produce stderr messages but don't crash.

**Non-requirements (left as exercises):** color output, recursive `-R`, tree view, `-t`/`-S` sorting, `-h` human-readable sizes, symlink decomposition, `-F` classification.

## File discovery — `fs::metadata` + `fs::read_dir`

The heart of the program is `find_files()`:

```rust
fn find_files(paths: &[PathBuf], show_hidden: bool) -> Result<Vec<PathBuf>> {
    let mut results = vec![];
    for name in paths {
        match fs::metadata(name) {
            Err(e) => eprintln!("{}: {e}", name.display()),       // report, continue
            Ok(meta) => {
                if meta.is_dir() {
                    for entry in fs::read_dir(name)? {
                        let entry = entry?;
                        let path = entry.path();
                        let is_hidden = path.file_name()
                            .map_or(false, |f| f.to_string_lossy().starts_with('.'));
                        if !is_hidden || show_hidden {
                            results.push(path);
                        }
                    }
                } else {
                    results.push(name.clone());
                }
            }
        }
    }
    Ok(results)
}
```

Three design points:

- **`fs::metadata(name)` first** — one syscall per argument to decide if it's a file or directory. If it's a file, push it directly. If it's a directory, read its contents with `fs::read_dir`. This matches `ls` behavior: `ls foo bar` succeeds for `bar` even if `foo` doesn't exist.
- **Error resilience** — a bad path prints to stderr but doesn't abort the loop. Same pattern as ch7's `findr` and ch9's `grepr`: report-and-continue.
- **Hidden file detection** — `path.file_name()` returns `Option<&OsStr>` (the last component, no parent path). `to_string_lossy().starts_with('.')` handles non-UTF-8 names gracefully. The `show_hidden` gate makes `-a` toggle this filter.

### Symlink subtlety — `metadata` vs `symlink_metadata`

`fs::metadata` follows symlinks: it reports the *target's* metadata, not the link's. If you want to know "is this entry itself a symlink?" you need `fs::symlink_metadata`, which reports the link's own metadata. The book's code uses `fs::metadata` throughout, which means a symlink to a directory is reported as `is_dir() == true` (the target's type), and a symlink's permissions are the target's permissions. This is fine for basic `ls` but breaks `ls -l` on symlinks (you'd see the target's mode, not `lrwxr-xr-x`).

A production `ls` uses `symlink_metadata` for the type/permissions and `read_link` to display the target:

```
lrwxr-xr-x 1 user staff 12 Jun 17 14:22 link -> target_file
```

## The long format — seven columns via `tabular`

```
-rw-r--r-- 1 user staff 193 Jun 17 14:22 bustle.txt
drwxr-xr-x 3 user staff  96 Jun 17 14:22 dir
┌──┐┌─────────┐ ┌──┐ ┌────┐ ┌─────┐ ┌───┐ ┌──────────────┐ ┌──────────┐
│d ││ rwxr-xr-x│ │ 3│ │user│ │staff│ │ 96│ │ Jun 17 14:22 │ │ bustle.txt│
└──┘└─────────┘ └──┘ └────┘ └─────┘ └───┘ └──────────────┘ └──────────┘
 1     2        3    4      5       6          7              8

 1 = file type char (d/-/l)
 2 = permission string (9 chars: rwxrwxrwx)
 3 = link count (nlink)
 4 = owner name (from uid via get_user_by_uid)
 5 = group name (from gid via get_group_by_gid)
 6 = size in bytes
 7 = modification time (chrono-formatted)
 8 = filename (path.display())
```

The `tabular` crate handles column alignment via a format string:

```rust
let fmt = "{:<}{:<} {:>} {:<} {:<} {:>} {:<} {:<}";
let mut table = Table::new(fmt);
```

`{:<}` = left-aligned, `{:>}` = right-aligned. The crate measures column widths from data, so narrow columns don't waste space and wide columns don't overflow. This is a real improvement over manual `format!` padding, which breaks when content widths change.

## Permission string formatting — bitmask decoding

Unix permissions are stored as a `u32` bitmask in the file's mode. The `format_mode()` function converts this into the familiar `rwxrwxrwx` string by testing three bits per owner class:

```rust
fn format_mode(mode: u32) -> String {
    format!("{}{}{}",
        mk_triple(mode, Owner::User),
        mk_triple(mode, Owner::Group),
        mk_triple(mode, Owner::Other),
    )
}

fn mk_triple(mode: u32, owner: Owner) -> String {
    let [read, write, execute] = owner.masks();
    format!("{}{}{}",
        if mode & read    != 0 { "r" } else { "-" },
        if mode & write   != 0 { "w" } else { "-" },
        if mode & execute != 0 { "x" } else { "-" },
    )
}
```

The `Owner` enum encodes the three octal digit positions:

```
┌─────────────────────────────────────────────────────────┐
│ mode bits (9 of them, plus 3 setuid/setgid/sticky):     │
│                                                         │
│         owner       group       other                   │
│         r w x       r w x       r w x                   │
│         4 2 1       4 2 1       4 2 1  (octal digit)    │
│         ─────       ─────       ─────                   │
│         0o400       0o040       0o004   (read mask)     │
│         0o200       0o020       0o002   (write mask)    │
│         0o100       0o010       0o001   (execute mask)  │
│                                                         │
│   e.g. 0o755 = rwxr-xr-x                                │
│        0o644 = rw-r--r--                                │
│        0o600 = rw-------                                │
└─────────────────────────────────────────────────────────┘
```

```rust
// owner.rs
#[derive(Clone, Copy)]
pub enum Owner { User, Group, Other }

impl Owner {
    pub fn masks(self) -> [u32; 3] {
        match self {
            Self::User  => [0o400, 0o200, 0o100],
            Self::Group => [0o040, 0o020, 0o010],
            Self::Other => [0o004, 0o002, 0o001],
        }
    }
}
```

The bitmask test `mode & mask != 0` is the canonical way to check a single bit. For `mode = 0o755`:

```
0o755 = 0b 111 101 101
      & 0b 100 000 000   (Owner::User read mask 0o400)
      = 0b 100 000 000   != 0  ->  "r"
```

The file type character (`d` for directory, `-` for regular file, `l` for symlink) is prepended separately in `format_output`, since it comes from a different check (`path.is_dir()`, `path.is_symlink()`), not from the permission bits.

## Owner and group lookup — the `users` crate

```rust
let uid = metadata.uid();
let user = get_user_by_uid(uid)
    .map(|u| u.name().to_string_lossy().into_owned())
    .unwrap_or_else(|| uid.to_string());

let gid = metadata.gid();
let group = get_group_by_gid(gid)
    .map(|g| g.name().to_string_lossy().into_owned())
    .unwrap_or_else(|| gid.to_string());
```

`metadata.uid()` and `metadata.gid()` come from `std::os::unix::fs::MetadataExt` — which makes this code Unix-specific. On macOS and Linux, `get_user_by_uid()` queries the user database (`getpwuid_r(3)`). If the UID has no corresponding entry (e.g. a container file owned by a non-existent user), we fall back to the numeric string. This is the right default: `ls` shows *something* for every file, never panics on a missing user.

### Unix-only — a portability caveat

`MetadataExt`, `users`, and `getpwuid_r` are Unix-only. A Windows port would need different permission and ownership APIs entirely — Windows has ACLs, not the `rwxrwxrwx` model. The book's `lsr` is explicitly a Unix `ls`; cross-platform is left as an exercise.

## Modernizations — symlinks, classification, batched stat

Three improvements worth calling out clearly (the book's existing notes mix these into the "Full Implementation" as if they were already implemented — they are not):

### 1. `fs::symlink_metadata` for correct symlink display

```rust
let metadata = fs::symlink_metadata(path)?;           // <- link's own metadata, not target's
let file_type = if path.is_symlink() { "l" }
                else if metadata.is_dir() { "d" }
                else { "-" };
```

This makes `ls -l` on a symlink show `lrwxr-xr-x` instead of the target's mode. Pair with `fs::read_link` to display the target:

```rust
let display_name = if path.is_symlink() {
    match fs::read_link(path) {
        Ok(target)    => format!("{} -> {}", path.display(), target.display()),
        Err(_)        => format!("{} -> (broken)", path.display()),
    }
} else {
    path.display().to_string()
};
```

### 2. `DirEntry::metadata()` to halve syscalls

The book calls `fs::metadata(name)` once per argument *and* `path.metadata()` once per entry — two syscalls per file. `fs::read_dir`'s `DirEntry::metadata()` returns metadata that the OS already cached during the directory scan on most platforms, halving the syscall count. For a directory with 10,000 entries this is a real speedup.

### 3. `EntryType` enum + `ls -F` classification

```rust
#[derive(Debug, PartialEq)]
enum EntryType { File, Dir, Symlink }

fn classify_entry(path: &Path) -> EntryType {
    if path.is_symlink() { EntryType::Symlink }
    else if path.is_dir() { EntryType::Dir }
    else { EntryType::File }
}

fn entry_indicator(path: &Path) -> &str {
    match classify_entry(path) {
        EntryType::File if is_executable(path) => "*",   // ls -F: executables get *
        EntryType::Dir                         => "/",
        EntryType::Symlink                     => "@",
        _                                      => "",
    }
}
```

These are *proposed* modernizations, not part of the author's source. The book's `lsr` is a faithful minimal port; a production `ls` would add them.

## Full implementation

```rust
mod owner;

use anyhow::Result;
use chrono::{DateTime, Local};
use clap::Parser;
use owner::Owner;
use std::{
    fs, os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
use tabular::{Row, Table};
use users::{get_group_by_gid, get_user_by_uid};

#[derive(Debug, Parser)]
#[command(author, version, about)]
/// Rust version of `ls`
struct Args {
    /// Files and/or directories
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,                                 // <- PathBuf, not String

    /// Long listing
    #[arg(short, long)]
    long: bool,

    /// Show all files
    #[arg(short('a'), long("all"))]
    show_hidden: bool,
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    let paths = find_files(&args.paths, args.show_hidden)?;
    if args.long {
        print!("{}", format_output(&paths)?);
    } else {
        for path in paths {
            println!("{}", path.display());
        }
    }
    Ok(())
}

fn find_files(paths: &[PathBuf], show_hidden: bool) -> Result<Vec<PathBuf>> {
    let mut results = vec![];
    for name in paths {
        match fs::metadata(name) {
            Err(e) => eprintln!("{}: {e}", name.display()),     // report, continue
            Ok(meta) => {
                if meta.is_dir() {
                    for entry in fs::read_dir(name)? {
                        let entry = entry?;
                        let path = entry.path();
                        let is_hidden = path.file_name()
                            .map_or(false, |f| f.to_string_lossy().starts_with('.'));
                        if !is_hidden || show_hidden {
                            results.push(path);
                        }
                    }
                } else {
                    results.push(name.clone());
                }
            }
        }
    }
    Ok(results)
}

fn format_output(paths: &[PathBuf]) -> Result<String> {
    // type perms links owner group size date name
    let fmt = "{:<}{:<} {:>} {:<} {:<} {:>} {:<} {:<}";
    let mut table = Table::new(fmt);

    for path in paths {
        // <- Modernization: symlink_metadata would show the link's own mode,
        //    not the target's. The book uses path.metadata() which follows links.
        let metadata = path.metadata()?;

        let uid = metadata.uid();
        let user = get_user_by_uid(uid)
            .map(|u| u.name().to_string_lossy().into_owned())
            .unwrap_or_else(|| uid.to_string());

        let gid = metadata.gid();
        let group = get_group_by_gid(gid)
            .map(|g| g.name().to_string_lossy().into_owned())
            .unwrap_or_else(|| gid.to_string());

        let file_type = if path.is_dir() { "d" } else { "-" };
        let perms = format_mode(metadata.mode());
        let modified: DateTime<Local> = DateTime::from(metadata.modified()?);

        table.add_row(
            Row::new()
                .with_cell(file_type)                            // 1
                .with_cell(perms)                                // 2
                .with_cell(metadata.nlink())                     // 3
                .with_cell(user)                                 // 4
                .with_cell(group)                                // 5
                .with_cell(metadata.len())                       // 6
                .with_cell(modified.format("%b %d %y %H:%M"))    // 7
                .with_cell(path.display()),                      // 8
        );
    }
    Ok(format!("{table}"))
}

// <- Bitmask-based permission formatting. Owner enum encapsulates the
//    three octal digit positions, making the bit tests self-documenting.
fn format_mode(mode: u32) -> String {
    format!("{}{}{}",
        mk_triple(mode, Owner::User),
        mk_triple(mode, Owner::Group),
        mk_triple(mode, Owner::Other),
    )
}

fn mk_triple(mode: u32, owner: Owner) -> String {
    let [read, write, execute] = owner.masks();
    format!("{}{}{}",
        if mode & read    != 0 { "r" } else { "-" },
        if mode & write   != 0 { "w" } else { "-" },
        if mode & execute != 0 { "x" } else { "-" },
    )
}
```

```rust
// owner.rs
#[derive(Clone, Copy)]
pub enum Owner { User, Group, Other }

impl Owner {
    pub fn masks(self) -> [u32; 3] {
        match self {
            Self::User  => [0o400, 0o200, 0o100],
            Self::Group => [0o040, 0o020, 0o010],
            Self::Other => [0o004, 0o002, 0o001],
        }
    }
}
```

## Key takeaways

- **`fs::metadata` follows symlinks; `fs::symlink_metadata` does not.** For `ls -l` on symlinks you need the link's own mode (`lrwxr-xr-x`), which means `symlink_metadata`. The book uses `metadata` throughout — a known limitation.
- **Unix permissions are a 9-bit bitmask** (3 bits × 3 owner classes), plus 3 high bits for setuid/setgid/sticky. `mode & mask != 0` is the canonical bit test. The `Owner` enum encodes the three octal digit positions, making `mk_triple` self-documenting.
- **`MetadataExt` (`uid()`, `gid()`, `mode()`, `nlink()`) is Unix-only.** The `users` crate (`get_user_by_uid`, `get_group_by_gid`) queries `getpwuid_r(3)`. A Windows port needs different APIs entirely — Windows has ACLs, not the `rwx` model.
- **The `tabular` crate handles column alignment** via a format string (`{:<}` left, `{:>}` right), measuring column widths from data. A real improvement over manual `format!` padding, which breaks when content widths change. (Modern alternatives: `tabled`, `comfy-table`.)
- **`DirEntry::metadata()` can halve syscalls** vs `path.metadata()` because the OS often caches entry metadata during `read_dir`. For large directories this is a real speedup.
- **Error resilience** — a bad path prints to stderr and the loop continues. Same report-and-continue pattern as ch7's `findr` and ch9's `grepr`. This is the Unix `ls` contract: `ls foo bar` succeeds for `bar` even if `foo` doesn't exist.
- **Hidden file detection** uses `path.file_name()` (the last component) + `starts_with('.')`. `file_name()` returns `Option<&OsStr>`; `to_string_lossy()` handles non-UTF-8 names gracefully.
- **`PathBuf` over `String`** for path arguments handles non-UTF-8 filenames without lossy conversion. clap's `PathBuf` value parser handles OS strings correctly.

---

> **Tying the book together & next steps:** This is the final chapter. For the meta-pattern that connects all 14 chapters to database internals, the single recommended next project (an LSM-tree KV store), the full 20-project roadmap (object stores → Arrow/Parquet → query engine → distributed KV → streaming), the Rust data-systems stack, and the reading list (DDIA → Database Internals → Raft/Dynamo papers) — see [`NEXT_STEPS_ROADMAP.md`](../NEXT_STEPS_ROADMAP.md) at the repo root.
