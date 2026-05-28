# Rust Crossword Fill Engine — Design

Status: implemented (M1–M4)

## 1. Goal and non-goals

**Goal.** A fast, quality-aware crossword fill engine that, given a structurally
valid grid skeleton and a scored wordlist, produces a *complete, high-quality*
fill of dense NYT-density grids (15×15, ~30–40 blocks, ~60–78 entries).

"Quality-aware" is the whole point and the thing every off-the-shelf filler we
reviewed lacks: it must prefer high-scoring answers and avoid crosswordese,
not merely find *some* valid fill.

**Concrete targets.**
- Throughput ≥ 200K nodes/sec single-thread.
- Fill a *valid* 30-block themeless at mean answer score ≥ 75 within ~5 s wall
  using ~8 parallel workers.
- Screen 1,000 candidate grids for fillability in < 60 s (grid selection).

**Non-goals (for the engine crate).**
- Clue writing, theme ideation, editorial QA — these are the Claude pipeline,
  a separate workstream.
- Grid *rendering* / gameplay — frontend concern (wrap Exolve).
- Wordlist *re-scoring* — a separate Claude batch job; the engine just consumes
  whatever scores it is handed.

## 2. Why fresh code (not forking crossword-composer)

paulgb/crossword-composer (MIT, Rust+WASM) is a clean reference but built around
the *opposite* tradeoff: it finds **any** valid fill from a tiny **flat**
(unscored) 15K word list, using a **static** solve order that lets it precompute
per-step lookup indexes. Our requirements — **quality scoring**, a **568K scored**
list, **dynamic** variable ordering, and **random restarts** — break the
assumption its index trick depends on (fixed known/unknown positions per step).

**Borrow:** (1) the geometry-agnostic constraint model (a puzzle = words as lists
of shared cell ids; a shared cell = a crossing), (2) the Rust→WASM packaging path,
(3) the permuted-index idea as a fallback if bitsets disappoint.

**Replace:** the search core, with bitset domains + dynamic MRV + score-ordered
enumeration + restarts + quality tiering + best-of search.

## 3. Core data model

Geometry-agnostic, following composer's insight:

- **Cell**: an atomic letter position, id `0..C`.
- **Entry** (composer calls it "word"): an ordered list of cell ids that must
  spell a dictionary word. id `0..E`.
- A cell shared by two entries is a **crossing**. The 2D grid is compiled down to
  this incidence structure; the solver never touches row/col geometry.

```rust
struct Puzzle {
    n_cells: usize,
    entries: Vec<Entry>,            // entry -> ordered cell ids
    cell_entries: Vec<Vec<(usize, usize)>>, // cell -> [(entry_id, pos_in_entry)]
    prefilled: Vec<Option<u8>>,     // cell -> Some(letter) for themers/rebus seed
}
struct Entry { cells: Vec<usize>, len: usize }
```

A 2D `Grid` module compiles a template (`.`/`#`/`A–Z`) into a `Puzzle`, enforcing
min run length ≥ 3. `prefilled` supports **theme entries** (pre-placed answers)
and rebus seeds from day one — important so we don't repaint later.

## 4. Wordlist and bitset domains (the performance core)

### 4.1 Layout
- Words grouped by length `L ∈ 3..=15`.
- Within each length, **sorted by score descending**, so word **index 0 = best**.
  This makes two things free:
  - **Score-ordered enumeration** = iterate set bits low→high.
  - **Quality tiers** = an index cutoff: "score ≥ T" ⇔ "index < cutoff[L][T]".
    Precompute `cutoff[L][tier]`.

### 4.2 Compatibility masks
For each `(L, position p, letter c)` precompute a bitset over the length-`L`
word indices: bit `i` set iff word `i` has letter `c` at position `p`.

```rust
// compat[L] indexed by (p * 26 + c) -> Bitset of length n_words[L]
struct Wordlist {
    words: Vec<Vec<Box<[u8]>>>,        // [L][idx] = letters
    scores: Vec<Vec<u8>>,              // [L][idx]
    compat: Vec<Vec<Bitset>>,          // [L][p*26+c]
    cutoff: Vec<Vec<u32>>,             // [L][tier] -> first index with score < tier
}
```
Memory estimate: ~5K masks × avg ~3 KB ≈ 15 MB. Fine. Build once, share via `Arc`.

`Bitset` = `Box<[u64]>` with `and_assign`, `count_ones` (hardware popcount),
`iter_ones`, and `andnot` (for used-word exclusion).

### 4.3 Per-entry live domains
Each entry holds a `Bitset` of still-possible word indices.
- Initial domain for entry of length L with prefilled letters = AND of the
  relevant `compat` masks (else all-ones over `n_words[L]`).
- **Used-word exclusion**: a global `used[L]` bitset; effective domain =
  `domain andnot used[L]`. Maintained incrementally on assign/undo.
- `domain_count[entry]` cached (popcount) for MRV.

## 5. Search algorithm

Dynamic backtracking with incremental domain maintenance.

```
solve(puzzle, wordlist, budget):
    init domains + counts (apply prefilled)
    recurse()

recurse():
    if all entries assigned: return SOLVED
    if nodes++ exceeds restart budget or deadline: abort attempt
    e = select_entry()            # MRV
    for w in ordered_candidates(e):   # score order (+ optional LCV), tier-limited
        trail = assign(e, w)          # set letters, AND neighbor domains, push undo
        if no neighbor domain empty:  # forward check
            if recurse() == SOLVED: return SOLVED
        undo(trail)
    return FAIL
```

### 5.1 Variable ordering — MRV
Pick the unassigned entry with the smallest in-tier domain count
(`popcount(domain andnot used & prefix[tier])`). Tie-break: most letters already
fixed. Linear scan over unassigned entries (~75) is negligible.

### 5.2 Value ordering — score, then optional LCV
Iterate the chosen entry's domain bits low→high (score desc), capped at the tier
cutoff. Optional **LCV refinement** on the top-K: for each candidate, sum the
resulting neighbor domain counts; prefer the least-constraining. Cheap in Rust
(K≈20 × ~4 neighbors × AND+popcount). Make it a tunable flag; start off, measure.

### 5.3 Assignment / undo (the trail)
On `assign(e, w)`:
- Set `e`'s cells to `w`'s letters; mark `w` used (`used[L] |= bit`).
- For each crossing neighbor `N` (unassigned): `domain[N] &= compat[lenN][posN][letter]`;
  push the *pre-AND* snapshot (or a delta) on the trail; recompute `domain_count[N]`.
- Forward-check: if any neighbor count hits 0, this candidate fails (still undo).

Undo pops the trail, restoring neighbor domains/counts and clearing the used bit.
Snapshot cost ≈ a few KB memcpy per neighbor — cheap in Rust. (If profiling shows
memcpy dominating, switch neighbor domains to a delta/inverted-AND scheme.)

### 5.4 Random restarts
Per attempt: a geometric node budget (2K, 4K, 8K, …). On exhaustion, restart from
scratch with a fresh seeded RNG. Randomness enters value ordering via a small
shuffle within equal-score bands so different attempts explore different regions.
Random restart is the single biggest lever for sparse/themeless grids.

### 5.5 Quality tiering + best-of
Outer loop over score floors (e.g. 70 → 60 → 50 → 40), spending a budget slice on
each. Higher tiers first keep fills clean; relax only if a tier can't solve in its
slice. Across all restarts, retain the **best solution by mean score** and return
it at the deadline. Stop early once a high tier yields a solution (relaxing would
only lower quality).

### 5.6 Parallelism
N worker threads (std::thread or rayon), distinct seeds, shared `Arc<Wordlist>`.
First acceptable solution cancels the rest, or run to deadline and take the global
best. Near-linear scaling; trivial in Rust.

## 6. Grid generation and selection

Two modules — the second is as important as the solver, because **not every valid
block pattern is fillable**; constructors discard the bad ones, and so must we.

- **Generator** (`gen::generate`): random symmetric block placement, reject runs
  < 3, require single connected white component, target block count.
- **Selector**: generate K candidates, run the solver with a short budget on each,
  keep those that fill at mean score ≥ threshold. Output a library of
  pre-validated, fillable daily-grid skeletons. Practical only at Rust speed.

## 7. Interfaces

Cargo workspace:
```
crates/
  xfill-core/   # lib: model, wordlist, domains, solver, grid gen/select. No I/O deps.
  xfill-cli/    # bin: load wordlist + template, solve, pretty-print. Dev/bench.
  xfill-wasm/   # cdylib (wasm-bindgen): browser-side fill. Optional, later milestone.
  xfill-svc/    # optional axum HTTP service for the app backend.
data/  templates/  benches/   # criterion benchmarks
```
- **Library API**: `Wordlist::load(path, min_score)`, `Puzzle::from_template(str)`,
  `Solver::new(&puzzle, &wordlist, cfg).solve() -> Option<Fill>`.
- **CLI** (`xfill`, `screen`, `library`) for filling, screening, and library builds.
- **WASM**: same engine, browser-side — lets the eventual UI offer live autofill.

## 8. Wordlist loading
Parse Crossword Nexus `WORD;score`: uppercase, alpha-only, length 3–15, drop
below `min_score`, dedupe. Group by length, sort by score desc, build `compat`
masks and `cutoff` tables. (Future: Claude-rescored scores swap in transparently —
same format.)

## 9. Risks / open questions
- **Bitset vs precomputed-index throughput.** Betting bitsets are plenty fast in
  Rust; if not, adopt composer's permuted-index for fixed sub-orders. Validate in M1.
- **Are random valid grids fillable?** Often not — hence grid *selection* is a
  first-class feature, not an afterthought.
- **Quality ceiling = wordlist scoring.** Saturday-grade fills may need Broda's
  finer scores or Claude re-scoring. Engine is agnostic; this is a parallel track.
- **Undo cost.** Snapshot trail is simple; switch to deltas only if profiling demands.

## 10. Milestones
- **M1 — Core + CLI.** Model, wordlist, bitset domains, single-thread DFS with MRV
  + score order. *Exit: ≥ 50× throughput; solves 38-block valid grids reliably.*
- **M2 — Restarts + tiering + best-of.** *Exit: valid 30-block themeless at mean
  ≥ 75 single-thread within budget.*
- **M3 — Parallel workers.** *Exit: that target in ~5 s wall, 8 threads.*
- **M4 — Grid generator + selector.** *Exit: emit a library of N fillable daily
  skeletons with quality stats.*
- **M5 — WASM + theme/pre-placed entries.** *Exit: browser-side fill demo; themers
  honored.*
