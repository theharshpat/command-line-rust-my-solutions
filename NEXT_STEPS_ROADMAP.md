# Next Steps Roadmap — From Command-Line Rust to Data Systems

> **Context:** You've finished (or are finishing) *Command-Line Rust* (this repo), have read TRPL, Rust by Example, Rust in Action, and done 100 Mainmatter exercises. This is your last Rust book for a while — next is conceptual reading (DDIA 2nd ed → *Database Internals* by Petrov) and project implementation / open-source contributions. This roadmap connects those worlds.

---

## Tying the book together

`lsr` (ch14) is a capstone in miniature: it composes filesystem traversal (ch7's `findr` pattern), metadata inspection, bitmask decoding (a small piece of systems programming that recurs in networking and kernel code), tabular output, and Unix-specific syscalls. The same mental model — *streams of records, filtered, transformed, formatted* — runs through every chapter from ch3's `catr` to here. Once you see `ls` as "iterate directory entries, extract metadata, format as rows," you have the template for every Unix utility and a surprising amount of systems software.

### The meta-pattern, and why it matters for databases

Every chapter in this book was the same shape, wearing different clothes:

```
   source ──► iterate ──► filter ──► transform ──► format ──► sink
```

That is *also* the shape of a query plan. The filesystem is a storage engine; `WalkDir`/`read_dir` is a table scan; `-name`/`-type`/`-n` are predicates; the iterator chain is the execution plan; `stdout` is the result sink. Ch7 said this explicitly; every other chapter reinforced it. The table below makes the mapping concrete so you can carry these reflexes into data-systems work:

| Book chapter | Unix tool     | Database / data-systems analogue                                                        |
|--------------|---------------|------------------------------------------------------------------------------------------|
| ch3 `catr`   | `cat`         | Full table scan, no predicate, project all columns                                       |
| ch4 `headr`  | `head`        | `LIMIT n` / early-termination scan                                                       |
| ch5 `wcr`    | `wc`          | Streaming aggregation (`COUNT`/`SUM`) without materializing the input                    |
| ch6 `uniqr`  | `uniq`        | Adjacent-key deduplication; the primitive behind sort-based distinct & merge             |
| ch7 `findr`  | `find`        | Filtered recursive scan = `SELECT * FROM tree WHERE predicates`; the query-engine frame |
| ch8 `cutr`   | `cut`         | Column projection (`SELECT col1, col3`); CSV/Parquet column extraction                   |
| ch9 `grepr`  | `grep`        | Predicate pushdown to the row level; linear-time regex = a WAF / log-filter rule         |
| ch10 `commr` | `comm`        | **Sort-merge join** on two pre-sorted inputs — the actual DB join algorithm              |
| ch11 `tailr` | `tail`        | `Seek`-based range scan; offset/limit semantics; the byte-mode seek = index lookup       |
| ch12 `fortuner` | `fortune`  | Seeded determinism = the test-fixture discipline for replayable query/chaos tests     |
| ch13 `calr`  | `cal`         | Calendar arithmetic; range encoding; grid layout (a small "render engine")              |
| ch14 `lsr`   | `ls`          | `pg_catalog` / information_schema; metadata scan + bitmask decoding + tabular render    |

And the *techniques* generalize too:

- **`Vec<Result<T>>` vs `Result<Vec<T>>`** (ch9) is exactly the partial-failure model in distributed queries where some shards/partitions error out but the rest still return.
- **Report-and-continue** (ch7, ch9, ch14) is the partial-availability contract of any multi-shard / multi-object fetch.
- **`mem::take` buffer reuse** (ch9) is the zero-copy / arena pattern that every high-throughput storage engine lives by.
- **Sort-merge lockstep** (ch10) is the inner loop of merge join, LSM compaction, and Kafka stream-table joins.
- **`Seek` vs two-pass count** (ch11) is the index-vs-scan decision a query planner makes for `LIMIT`/`OFFSET`.
- **Streaming vs collect** (ch7, ch9, ch11) is the materialization-vs-pipelining tension at the heart of every query executor (DataFusion streams Arrow `RecordBatch`es; Materialize streams differential dataflow).
- **Bitmask decoding** (ch14) is the same skill for page headers, WAL record types, and bloom-filter probes.
- **Type-level proofs (`NonZeroUsize`, `Range<usize>`, half-open indexing)** (ch8, ch11) is the "make illegal states unrepresentable" discipline you want for page IDs, segment IDs, epoch numbers.

---

## If you had to pick one project: build a single-node LSM-tree KV store

**Why this one** — it's the literal implementation of DDIA Ch3 (*Storage and Retrieval*) and the entire first half of *Database Internals* (Petrov). You read the chapter, then you build it, and the intuition becomes permanent. Every other interesting distributed project (P15 sharding, P16 Raft, P17 distributed SQL) is a thin layer on top of a working LSM.

And it consumes everything you just learned in Command-Line Rust:

- **ch5 `wcr`** — streaming count for memtable threshold + size checks
- **ch7 `findr`** — `WalkDir` + `.dat`-style extension filter becomes SSTable discovery on startup
- **ch9 `grepr`** — `mem::take` buffer reuse; `Vec<Result<T>>` for multi-file reads where some SSTables fail to open
- **ch10 `commr`** — the sort-merge lockstep **is**, with zero changes, the **leveled compaction algorithm**. Once you've built a merge join in ch10, you have the compaction loop for free
- **ch11 `tailr`** — the WAL is exactly an append-only file with `SeekFrom::End` for recovery
- **ch14 `lsr`** — bitmasks return as Bloom filter implementation; page-header-style byte packing for SSTable index blocks

### Build it in this order (each step is independently testable)

1. **WAL**: append-only file with CRC + length-prefixed records. `PUT key value`, `DELETE key` (tombstone). `fsync` on every write.
2. **Memtable**: `BTreeMap<Vec<u8>, Vec<u8>>`. Reads/writes go through it.
3. **Flush**: when memtable exceeds N bytes, freeze it and write an immutable SSTable (sorted keys + sparse index).
4. **Read path**: memtable → L0 SSTables (newest first) → L1...
5. **Compaction**: when L0 reaches K files, merge them via the ch10 algorithm into L1. Old files deleted.
6. **Bloom filters**: one per SSTable, bitset, tested on every L1+ read.
7. **Startup recovery**: scan WAL (ch11) + discover SSTable files (ch7).

Crates you need: `std`, `parking_lot` (for the `RwLock` around memtable), `crc32fast`. That's it — no Arrow, no async, no networking. Pure std + 2 deps. Maybe 800-1200 lines when you're done, which is a small enough surface to actually finish and feel ownership of.

### The thing reading will never give you

The *feel* of write amplification vs read amplification vs space amplification. When you fire a workload and your read latency triples during compaction, that "aha" is worth more than rereading the chapter three times. Pet theories about why RocksDB has 50 tunable knobs evaporate — you'll discover you need them yourself.

### Read alongside

The [Bitcask paper](https://riak.com/assets/bitcask-intro.pdf) (for the simpler version) + `fjall` source (the maintained pure-Rust LSM you're effectively building a tiny version of). After this, contributing to `fjall` or jumping to `rust-rocksdb` + TiKV is realistic — you'll recognize every component.

### The natural extension

Not part of "the one thing," but the obvious next move once you have it: wrap it in `tonic` gRPC, plug `openraft` in for replication, and you have a 3-node distributed KV — because now the "storage engine" half is already done and you're only adding the distributed half. But that's a separate project; the LSM is the load-bearing piece.

---

## The Rust data-systems stack to learn next

The single most important thing to internalize: **Apache Arrow + DataFusion is the foundation of the modern Rust data-systems world.** InfluxDB IOx, GreptimeDB, GlareDB, Ballista, LanceDB, DataFusion itself, and large parts of RisingWave all sit on it. Learning it pays compounding interest.

- **`arrow`** — columnar in-memory format. The `RecordBatch` is the unit of streaming data through every modern Rust query engine. Learn the array types (`StringArray`, `PrimitiveArray`, `BooleanArray`), validity bitmaps, and zero-copy slicing.
- **`parquet`** — columnar on-disk format. Row groups, column chunks, page indexing, predicate pushdown, bloom filters. This is the modern "filesystem for analytics."
- **datafusion** — the query engine: logical plans, physical plans, optimizers, streaming execution via `RecordBatchStream`. Read its source; it's well-organized and is a textbook execution engine.
- **`object_store`** (from the Arrow org) — the S3/Azure/GCS/HTTP/local abstraction. This is the *cloud-native filesystem*. `ls`/`cat`/`find` over S3 are exactly what `aws s3 ls`, `s5cmd`, and the `object_store` API do — the Unix philosophy generalizes to object storage. Build a `findr`-equivalent over `object_store` as a first real project.
- **`opendal`** — a more general data-access layer (fs, S3, GCS, Azure, FTP, HDFS, Redis, …) under very active development. Good second choice after `object_store`.
- **Embedded storage engines** (read source, then contribute):
  - `redb` — pure-Rust embedded KV, clean code, good for learning B-trees + MVCC.
  - `fjall` — pure-Rust LSM-tree KV, actively maintained, excellent LSM reading.
  - `rust-rocksdb` — bindings to RocksDB; ubiquitous in production (TiKV, many others).
  - `sled` — historically important, educational, but check maintenance status before depending on it.
- **Distributed primitives**:
  - `openraft` — the maintained Raft implementation (rust-rocksdb + openraft + tonic gets you 80% of a distributed KV store).
  - `raft-rs` — the TiKV-origin Raft.
  - `tonic` + `prost` — gRPC + protobuf, the lingua franca of distributed systems.
  - `tokio` — async runtime; non-negotiable for any networked data system.
- **Streaming**:
  - `timely-dataflow` + `differential-dataflow` — Frank McSherry's work; the engine behind Materialize. Deep, research-flavored, very rewarding.
- **Testing distributed systems in Rust** (this is where Rust truly shines vs Go/Java):
  - `madsim` — deterministic simulator (used by RisingWave). Lets you run a whole distributed cluster in one process, inject failures deterministically, and replay. This is how you get correctness in distributed systems without a 50-node test harness.
  - `loom` — concurrency model checker for `Arc`/`Mutex`/`channel` code.
  - `shuttle` — randomized testing of concurrent code (used by Materialize).

---

## A progressive project ladder — 20 projects, 6 phases

Each project below is small enough to actually finish, builds on the previous one (and on specific book chapters), and targets a concept you'll need for data-systems work. Do them in order — skills compound.

### Phase 0 — Reuse the book's reflexes, swap the source

Goal: make the local-filesystem reflexes async and source-agnostic. Weekend projects each, but load-bearing — they teach `tokio`, `bytes::Bytes`, the `object_store` trait abstraction, and the "object store as filesystem" mindset that every modern data system uses.

**P1. `findr-s3` — `find` over an object store** *(builds on ch7 `findr`)*
- Reimplement `findr` with `object_store::ObjectStore` (the trait) instead of `walkdir`. Support `LocalFileSystem`, `AmazonS3`, `HTTP`. Keep `-n` (regex on object key basename), add `-p`/`--prefix` (the S3 analogue of "directory"). Stream matches to stdout.
- Crates: `object_store`, `tokio`, `bytes`, `regex`, `clap`.
- Teaches: async iterators (`StreamExt`), the `ObjectStore` trait as a uniform source, `bytes::Bytes` zero-copy, pagination over list-objects-v2.
- Stretch: add `--format jsonl` and `--format arrow` output modes — same data, different sink.

**P2. `s3ls` — `ls` over an object store** *(builds on ch14 `lsr`)*
- Long-format listing of objects: key, size, last-modified, ETag, storage class. Use `tabled` (modern replacement for `tabular`).
- Teaches: object metadata model vs Unix inode model; what `ls -l` concepts survive and which don't (no permissions, no nlink, no owner — S3 has buckets and ACLs instead).

**P3. `s3tail` — `tail` from object storage via byte-range GETs** *(builds on ch11 `tailr`)*
- `tail -c N s3://bucket/key` using HTTP `Range: bytes=-N` headers — this is the *real* version of ch11's byte-mode `Seek`, and it's how every cloud log viewer works. Extend to "tail across all objects under a prefix, ordered by LastModified" — a streaming merge.
- Teaches: byte-range GETs (the object-store analogue of `Seek`), streaming merge of multiple sources (ch10's merge algorithm, but on object streams instead of files).

**P4. `s3grep` — `grep` over object storage with parallelism** *(builds on ch9 `grepr`)*
- Parallel scan: list objects under a prefix, fan out across a `tokio` task group, grep each, merge results. Keep the `Vec<Result<T>>` partial-failure contract — some objects may 403 or 404, the rest must still return.
- Teaches: bounded parallelism (`Semaphore` / `buffer_unordered`), partial-failure at network scale, the pattern that becomes a distributed scan in a real query engine.

### Phase 1 — Structured formats (Arrow + Parquet)

Goal: stop working with text lines; start working with columnar records. This is where the "typed records" framing of the book becomes literal.

**P5. CSV → Arrow → Parquet converter** *(builds on ch8 `cutr`)*
- Read a CSV with `csv` + `arrow-csv`, build `RecordBatch`es, write Parquet with `parquet`. Add `--schema file.json` to override inferred types. Stream batches so a 10GB CSV doesn't load into memory.
- Crates: `arrow`, `arrow-csv`, `parquet`, `tokio`.
- Teaches: Arrow's columnar memory layout, `RecordBatch` as the streaming unit, schema inference and overrides, dictionary encoding for low-cardinality strings.
- Stretch: add `--row-group-size N` and watch Parquet file size + scan speed change.

**P6. `pqcat` — `cat` for Parquet files** *(builds on ch3 `catr`)*
- `pqcat file.parquet` prints rows; `pqcat --schema file.parquet` prints the schema; `pqcat --row-groups file.parquet` lists row groups with column stats (min/max/null_count). This is the "metadata inspector" you'll reach for constantly.
- Teaches: Parquet file layout (row groups → column chunks → pages), column statistics (the basis for predicate pushdown in P8).

**P7. `pqcut` — column projection over Parquet (the *real* `cut`)** *(builds on ch8 `cutr`)*
- `pqcut --columns a,c file.parquet` reads only the requested column chunks from disk. Show that it's dramatically faster than reading all columns.
- Teaches: columnar I/O economics — reading 3 of 50 columns is ~3/50 the bytes, not ~all of them. This is *the* reason columnar formats exist, and seeing it measured makes it stick.

### Phase 2 — A tiny query engine (the heart of the matter)

Goal: build a ~1000-line query engine on top of Arrow + Parquet. This is where ch7's "find is a query engine" framing stops being a metaphor.

**P8. SQL `SELECT ... WHERE` over Parquet, by hand** *(builds on ch9 `grepr` + ch11 `tailr`)*
- Parse a *very* small SQL subset (`SELECT col1, col2 FROM file WHERE col3 > 10 LIMIT 100`) with `sqlparser-rs`, build a physical plan: `ParquetExec → FilterExec → ProjectionExec → LimitExec → Collect`. Run it. No optimizer yet.
- Crates: `sqlparser-rs`, `arrow`, `parquet`, `tokio`.
- Teaches: logical vs physical plans, the executor pattern, `Send` `Stream<Item = Result<RecordBatch>>` as the universal interface.
- Stretch: add Parquet predicate pushdown — if `WHERE col3 > 10` and a row group's max for `col3` is 5, skip the whole row group. This is the *real* version of "skip the bad record" from ch7's report-and-continue.

**P9. Add aggregation: `GROUP BY` + `COUNT`/`SUM`/`AVG`** *(builds on ch5 `wcr`)*
- Hash aggregate operator. Stream input batches, accumulate into a `HashMap<RowKey, Accumulator>`, emit one output batch at end. Then add streaming `GROUP BY` with spill-to-disk when the map exceeds memory — same pattern as ch11's "two-pass for lines, one-pass for bytes" but for memory.
- Teaches: hash aggregation, memory-bounded execution, the aggregator state machine. This is *exactly* what DataFusion's `AggregateExec` does.

**P10. Add the sort-merge join — ch10 revisited, for real** *(builds on ch10 `commr`)*
- Sort both inputs on the join key (reuse a sort operator from P9's spill infra), then merge-join in lockstep emitting matched pairs as new `RecordBatch`es. This is the canonical outer-loop of every OLAP join.
- Teaches: external sort (spill + merge), the ch10 algorithm at batch granularity, build/probe vs sort-merge tradeoffs.

**P11. Now throw away P8–P10 and rewrite on DataFusion** *(builds on everything in P8–P10)*
- Take your SQL parser from P8 and hand the AST to DataFusion's `LogicalPlan` builder. Use DataFusion's operators, optimizers, and execution. Compare your P8–P10 implementations to DataFusion's — read the diffs. This is the most valuable reading you'll do.
- Teaches: what a production query engine looks like, the optimizer rules, `Environments` / `SessionContext` / `TableProvider`. After this, *contributing* to DataFusion is realistic.

### Phase 3 — Embedded storage engine

Goal: build the storage layer that sits under query engines. P8–P11 read immutable files; now make data writable.

**P12. Bitcask — a 300-line log-structured KV** *(builds on ch11 `tailr` + ch5 `wcr`)*
- Implement the [Bitcask paper](https://riak.com/assets/bitcask-intro.pdf) exactly. Append-only data files, in-memory keydir (HashMap key → `{file_id, offset, size}`), startup recovery by scanning all files. CRUD API.
- Crates: `std::fs`, `parking_lot::Mutex`, `crc32fast` (checksums — important for WAL integrity).
- Teaches: write-ahead log, append-only storage, in-memory index, crash recovery, the simplest correct storage engine that exists. Read `redb` source after to see how a real B-tree engine differs.
- Stretch: add merge compaction to reclaim space from overwritten/deleted keys.

**P13. LSM-tree — `fjall`-in-miniature** *(builds on P12 + ch10 `commr`)*
- Memtable (sorted mem `BTreeMap`), flush to SSTable on size threshold, Leveled or Tiered compaction merging SSTables via the ch10 sort-merge algorithm. Bloom filters on SSTables (bitmask skills from ch14 return here). `GET` checks memtable → L0 → L1 → …
- Teaches: LSM architecture (the foundation of RocksDB, Cassandra, LevelDB, fjall), compaction as sort-merge, Bloom filters, tombstones, read amplification vs write amplification vs space amplification.
- Stretch: add snapshot reads using sequence numbers (MVCC lite).

**P14. MVCC + serializable transactions on the LSM** *(builds on P13)*
- Add per-key version chains (sequence numbers), `BEGIN/COMMIT/ROLLBACK`, snapshot isolation, then serializable via SSI or OCC. Write a concurrency test suite using `loom` to verify isolation.
- Teaches: MVCC, isolation levels, OCC vs 2PL, the testing discipline that makes concurrency-correct code possible.
- Read alongside: TiKV's `storage` module, Postgres's `HeapTupleSatisfiesMVCC`.

### Phase 4 — Distributed

Goal: take the embedded engine and make it survive node failure.

**P15. Sharded KV with a pluggable router** *(builds on P12/P13 + ch9 `grepr`)*
- Run N instances of your P13 LSM as separate processes. A router maps `key → shard` via consistent hashing. `PUT`/`GET` route accordingly. Build a chaos test harness that kills random shards and asserts the surviving keys still serve.
- Crates: `tonic` (gRPC), `prost` (protobuf), `tokio`, `clap`.
- Teaches: consistent hashing, sharding, partial availability, idempotent retries.
- This is the first project where ch9's `Vec<Result<T>>` is literally the right return type for a multi-shard scan.

**P16. Raft replication with `openraft`** *(builds on P15)*
- Replace each "shard" in P15 with a 3-node Raft group using `openraft`. Writes go through the leader; reads can go to followers (with stale-read semantics documented). Add leader-election chaos tests.
- Crates: `openraft`, `tokio`, `tonic`.
- Teaches: Raft (leader election, log replication, snapshots), the strong-consistency vs availability tradeoff, log shipping as the replication primitive.
- Read alongside: the Raft paper, then `openraft` source.

**P17. Distributed SQL frontend on top of P15/P16** *(builds on P11 + P15/P16)*
- Implement a `TableProvider` for DataFusion that scans your distributed KV / LSM as a "table." Write a SQL `SELECT` planner that turns a range scan into parallel `GET`s across shards, merges results as `RecordBatch`es. This is a *distributed query executor in 500 lines*.
- Teaches: the integration of query engine + distributed storage — exactly the architecture of TiKV + TiDB, CockroachDB, etc.

### Phase 5 — Streaming

Goal: replace batch scans with continuous flows.

**P18. Kafka → filter → Parquet-on-S3 ETL** *(builds on ch9 `grepr` + P5 + P1)*
- Consume a Kafka topic (`rdkafka`), apply a filter expression, buffer `RecordBatch`es, roll to a new Parquet file on S3 every N rows or M minutes. At-least-once semantics first; then upgrade to exactly-once via transactions.
- Crates: `rdkafka`, `arrow`, `parquet`, `object_store`, `tokio`.
- Teaches: stream processing, micro-batching, exactly-once semantics, checkpointing.
- This is the real-world version of ch9's "stream, don't collect" takeaway.

**P19. Windowed aggregation stream** *(builds on P18 + P9)*
- Tumbling and hopping windows over the Kafka stream from P18, with watermark-based late-event handling. Emit aggregate Parquet files per window.
- Teaches: event-time vs processing-time, watermarks, windowing, late events — the core concepts of Flink/Spark Structured Streaming/Materialize.

**P20. Materialized view engine (tiny Materialize)** *(builds on P19)*
- Maintain an in-memory materialized view (a `HashMap` keyed by group key) updated incrementally as events arrive. Expose `SELECT * FROM mv` via your P11 DataFusion integration. This is *exactly* what Materialize does, minus the differential-dataflow sophistication.
- Teaches: incremental view maintenance, the difference between recomputation and incremental update.
- Then read: `differential-dataflow` source — the same idea, generalized to collections with multiplicities.

### Suggested pace and order

If you're working through these alongside a full-time job:

- **Months 1–2:** Phase 0 (P1–P4) — solidify the book's patterns in async + cloud-native form.
- **Months 3–4:** Phase 1 (P5–P7) — Arrow + Parquet fluency. Non-negotiable foundation.
- **Months 5–7:** Phase 2 (P8–P11) — the query engine. P11 (rewrite on DataFusion) is the most valuable single project on this list.
- **Months 8–10:** Phase 3 (P12–P14) — storage engine. P13 (LSM) is where database internals finally click.
- **Months 11–14:** Phase 4 (P15–P17) — distributed. P17 ties engine + storage + distribution together.
- **Months 15+:** Phase 5 (P18–P20) — streaming, if that's where you want to specialize.

If you want to go faster, **the single highest-leverage path is P1 → P5 → P7 → P8 → P11**: object store → Arrow → Parquet column projection → hand-rolled SQL executor → DataFusion. That's the spine of modern analytics engineering in Rust, and you can do it in ~2 months of evenings.

### A meta-tip on *how* to build these

- **Write the integration test first** — for every project, write the CLI behavior you want *before* the implementation. The book trained you in this via `tests/cli.rs`; keep the habit.
- **Use `madsim` or `loom` for anything concurrent** — don't trust your concurrent code until a model checker has run it. Rust's value here is unique.
- **Read one real codebase alongside each phase** — Phase 0/1: `object_store` source. Phase 2: DataFusion `core/`. Phase 3: `fjall` then `redb`. Phase 4: `openraft` then TiKV `raftstore`. Phase 5: `risingwave` streaming operators. Reading production code *while building a toy version* is the fastest way to absorb the concepts.
- **Write a postmortem after each project** — what surprised you, what was harder than expected, what you'd do differently. The postmortem is where the learning cements.

---

## Projects to read (in roughly increasing scope)

1. **`redb`** — a few thousand lines, pure Rust, B-tree + MVCC. Start here for "how does an embedded DB work."
2. **`fjall`** — LSM-tree, flush/compaction, SSTables. Pairs with *Database Internals* (Petrov) chapters on LSM.
3. **`object_store`** — small, clean trait-based abstraction over cloud storage. Read it, then write a `findr` clone over it.
4. **DataFusion** — query planning, physical operators, streaming execution. The `core/` directory is a full execution engine.
5. **`lancedb` / `lance`** — vector search on object storage; modern, multi-modal, Rust-native.
6. **TiKV** — production distributed transactional KV (Raft + MVCC + Percolator-style transactions). Large, but the `raftstore` and `storage` modules are the canonical reference.
7. **RisingWave** — streaming database; uses DataFusion + its own streaming engine + `madsim` for deterministic testing. State-of-the-art Rust distributed systems engineering.

---

## Reading list (concepts first, then Rust-flavored)

- **Martin Kleppmann, *Designing Data-Intensive Applications*** (2nd ed) — the bible. Read it before anything else; every concept (replication, partitioning, consistency, query, stream) is here.
- **Alex Petrov, *Database Internals*** — storage engines, indexing, B-trees vs LSM, transaction logs. Maps directly onto `redb`/`fjall` source.
- **The Raft paper** (Ongaro & Ousterhout, 2014) — then read `openraft` source alongside it.
- **The Dynamo paper** (DeCandia et al., SOSP 2007) — eventually-consistent distributed KV.
- **Google papers** — GFS (2003), Bigtable (2006), Spanner (2012), Percolator (2010). Spanner/Percolator map onto TiKV's transaction model.
- **The LMAX Disruptor paper** — high-throughput ring-buffer queues; relevant to any streaming engine.
- **DataFusion documentation + the "Building a Database with DataFusion" talks** — the practical on-ramp.
- **Andy Pavlo's CMU database lectures** (YouTube) — query optimization, storage, internals; language-agnostic but the mental models transfer cleanly to Rust.

---

This is the whole book's thesis in one sentence: **the Unix philosophy is not about small tools for their own sake; it is about composable streaming transformations over typed records, and Rust's `Iterator` + `Result` + trait-object machinery is the cleanest modern expression of it.** Every database, every query engine, every log processor, every streaming pipeline is that same shape at a different scale. You now have the reflexes; the next step is just pointing them at bigger stores.
