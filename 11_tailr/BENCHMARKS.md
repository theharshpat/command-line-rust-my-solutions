# Benchmark Report: `tailr` vs Book's `tailr`

Benchmarking the solution against the system `tail` and the book's reference
implementation, on a 1M-line (55 MB) randomly generated text file.

## Setup

- **Input file**: `1M.txt` — 1,000,000 lines of randomly generated text (55 MB),
  produced by the author's `biggie` utility (`cargo run --release -- -l 1000000 -o 1M.txt`).
- **Our binary**: `cargo build --release` → `target/release/tailr`
- **Reference**: macOS BSD `tail`
- **Tool**: `hyperfine -i -N --warmup 3 -L prg tail,target/release/tailr '{prg} ... > /dev/null'`
- **Hardware**: macOS (darwin). Book's numbers are from a different machine —
  so absolute times across the two are not comparable. To compare our `tailr` to
  the book's `tailr`, I normalize each to **BSD `tail` on the same machine** and
  take the ratio. This isolates algorithmic differences from hardware.

## The book's `tailr` (always forward-scan)

The book's implementation always performs a **full forward scan**: count every
line or byte, then `seek` to the start offset and print. As a result it is
dramatically slower than system `tail` for everything except the one case where
`tail` itself also pays to read the whole file (`-c 1000000`).

## Our `tailr` (reverse-scan / metadata-driven)

Three algorithmic changes flip the comparison:

1. **Reverse scan for `-n N` (last N lines)** — `seek` to EOF, read 8 KB blocks
   backwards counting `\n`, find the start byte, `io::copy` to stdout.
   O(last N lines) instead of O(file_size).
2. **`metadata().len()` for `-c` (byte mode)** — one `stat()` syscall gives the
   total file size; then `seek(file_size - n)` and `io::copy`. No file scan.
3. **Forward scan only for `-n +N` (skip first N)** — read once counting lines
   to the start, then `io::copy` the remainder. Single pass, kernel-buffered.

## Book's 4 scenarios

Normalized to BSD `tail` on the same machine. "x of tail" = `tailr_time / tail_time`.
Lower is better; <1 means faster than `tail`.

| Scenario | Book: tailr vs tail | Ours: tailr vs tail | Ours vs Book's tailr |
|---|---|---|---|
| default (last 10) | 20.32x slower | 1.07x slower (parity) | **19.0x faster** |
| `-n 100000` (last 100K lines) | 5.78x slower | 0.50x (2.0x faster) | **11.6x faster** |
| `-c 100` (last 100 bytes) | 14.98x slower | 1.07x slower (parity) | **14.0x faster** |
| `-c 1000000` (last 1M bytes) | 1.34x faster | 17.19x faster | **12.8x faster** |

### Same data, absolute times

Book's run (BSD `tail` on its host):

| Scenario | BSD `tail` | Book's `tailr` | Ours: `tailr` (our host) | BSD `tail` (our host) |
|---|---|---|---|---|
| default (last 10) | 4.3 ms | 86.9 ms | 1.5 ms | 1.4 ms |
| `-n 100000` | 26.8 ms | 154.7 ms | 6.2 ms | 12.4 ms |
| `-c 100` | — | 14.98x slower | 1.5 ms | 1.4 ms |
| `-c 1000000` | — | 1.34x faster | 1.7 ms | 29.3 ms |

## Extra scenarios (beyond the book)

The book did not benchmark these. They illustrate where the optimization pays off
most clearly. Absolute times, our hardware only:

| Scenario | BSD `tail` | Ours: `tailr` | Ours vs tail |
|---|---|---|---|
| `-n 1` (last 1 line) | 1.4 ms | 1.5 ms | 1.10x slower (parity) |
| `-n 1000000` (last 1M = whole file) | 106.7 ms | 48.1 ms | **2.22x faster** |
| `-n +500000` (skip first 500K) | 1208 ms | 280.7 ms | **4.30x faster** |

## Why the book's `tailr` is so much slower

The book's `get_start_index` + `count_lines_bytes` design reads the **entire file**
to count lines/bytes, then `seek`s and reads again to print. On a 55 MB file that
is two passes through the whole file for *every* invocation — including `tail 1M.txt`
(last 10 lines) and `tail -c 100 1M.txt` (last 100 bytes), where only a tiny tail
is actually needed.

### `default` (last 10 lines)
- Book: forward-scan all 1M lines, then seek and print last 10. ~87 ms.
- Ours: reverse-scan ~8 KB from EOF, find the 10th newline from the end, copy
  ~600 bytes to stdout. ~1.5 ms.
- **19x faster.**

### `-n 100000` (last 100K lines)
- Book: forward-scan all 1M lines, then seek and print last 100K. ~155 ms.
- Ours: reverse-scan ~5.5 MB from EOF (the last 100K lines), then `io::copy` them
  to stdout. ~6.2 ms.
- **11.6x faster.**

### `-c 100` (last 100 bytes)
- Book: forward-scan all 55 MB to count bytes — even though the file size is
  available from `f.metadata().unwrap().len()` in one syscall.
- Ours: `metadata().len()` → `seek(len - 100)` → `io::copy` 100 bytes. ~1.5 ms.
- **14x faster.**

### `-c 1000000` (last 1M bytes)
- Book: still scans the whole file forward, but here `tail` also has to write
  1 MB, so the book's overhead is partly hidden — it manages to edge out `tail`
  by 1.34x.
- Ours: `metadata().len()` → `seek(len - 1_000_000)` → `io::copy` 1 MB. No scan.
  17x faster than `tail`, **12.8x faster than the book's tailr.**

## Notes & caveats

- Sub-millisecond measurements have high relative variance; the "1.1x slower"
  cases (default, `-n 1`, `-c 100`) are **at parity** with `tail` — the
  difference is dominated by process startup, not by the algorithm. The book's
  tailr was 15-20x slower on these same cases precisely because its algorithm
  dominated startup.
- The book and we run on different hosts, so **absolute times across hosts are
  not directly comparable**. The "Ours vs Book's tailr" column is computed from
  the normalized-to-`tail` ratios, which strips out the hardware factor and
  isolates the algorithmic speedup. This assumes BSD `tail`'s own implementation
  behaves similarly across hosts, which is reasonable for these operations.
- `-n +N` (skip first N lines) genuinely needs a forward scan — there is no way
  to find "line K from the start" without reading from the start. Our 4.3x win
  over `tail` here is just from using `read_until` + `io::copy` (single pass,
  kernel-managed buffer) rather than per-line allocation. The book's tailr would
  also need a forward scan here, so the win comes from the same single-pass
  strategy — not from the reverse-scan optimization.

## Assets preserved

- `11_tailr/verify/inputs/` — 17 edge-case input files for the byte-level
  verification test in `tests/verify_tail.rs`. Kept; do not delete.