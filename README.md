# xword-pipeline

[![CI](https://github.com/ekorbia/xword-pipeline/actions/workflows/ci.yml/badge.svg)](https://github.com/ekorbia/xword-pipeline/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

An end-to-end pipeline for generating **dense, NYT-style crossword puzzles** with
AI-written clues and an AI editorial pass.

```
  theme-idea ─▶  fill-engine  ─▶  clue  ─▶  qa  ⇄  clue --revise
  (optional)     (Rust: grid     (Claude:   (Claude:   (Claude: fix
   theme sets     generation +    write      review)    flagged clues)
                  scored fill)    clues)
```

Two components live side by side under this repo:

```
xword-pipeline/
├── run-pipeline.sh      # one command: generate → clue → qa
├── fill-engine/         # Rust: dense grid generation + quality-scored fill
├── clue-writer/         # TypeScript: Claude clue writing, QA, theme ideation
└── out/                 # ALL generated JSON (gitignored)
    ├── libraries/       #   grid-library.json / theme-library.json   (fill-engine)
    ├── puzzles/         #   *.clued.json, *.qa.json, *.revised.json   (clue/qa/revise)
    └── themes/          #   theme-idea output
```

The fill engine is pure Rust and runs offline (free). The clue/QA/theme steps
call **Claude Opus 4.7** and need an API key.

---

## One-time setup

```bash
# 1. Build the Rust engine
(cd fill-engine && cargo build --release)

# 2. Install the Claude layer
(cd clue-writer && npm install)

# 3. Set your Anthropic API key (see clue-writer/README for how to get one)
export ANTHROPIC_API_KEY=sk-ant-...
```

---

## Quick start — the whole pipeline in one command

`run-pipeline.sh` runs **generate → clue → qa** and prints the QA verdict. All
output lands in `out/`. (Fixing the findings it reports is a separate manual
step — see [Fixing QA findings](#fixing-qa-findings).)

```bash
# Themeless (Fri/Sat-style) puzzle
./run-pipeline.sh --mode themeless --day Saturday

# Mini puzzles — small grids that are quick to fill & solve (great for testing)
./run-pipeline.sh --mode themeless --size 5  --day Easy     # 5×5 mini
./run-pipeline.sh --mode themeless --size 10 --day Medium   # 10×10 midi

# Themed (Mon–Thu-style) puzzle — supply the theme answers
./run-pipeline.sh --mode themed \
  --themes WAITINGFORGODOT,ROCKETSCIENCE,TROMBONE --day Wednesday
```

The grid step is free; the script pauses before the paid Claude steps if
`ANTHROPIC_API_KEY` isn't set (your library is still generated). On completion it
prints the three artifact paths and the verdict.

### Script flags

| Flag | Default | Notes |
|------|---------|-------|
| `--mode themeless\|themed` | `themeless` | Themed requires `--themes` |
| `--size N` | `15` | **Themeless only.** Grid dimension — e.g. `5` or `10` for minis. Themed grids are 15×15 for now |
| `--themes A,B,C` | — | Theme answers (A–Z, no spaces). 1–4 answers; each 3–15 letters but **never exactly 12** (a 12-letter answer can't be placed as a single across in a 15-wide grid) |
| `--blocks N` | ~16% of area / 44 | Black squares. Defaults to ~16% of the grid area (15→36, 10→16, 5→4); themed grids want **more** (42–44) to ease the fill |
| `--day DAY` | Saturday / Wednesday | Clue difficulty. Accepts a day (Monday…Saturday) **or** a friendly word: `Easy`=Mon, `Medium`=Wed, `Tricky`=Thu, `Hard`=Fri, `Expert`=Sat |
| `--grid N` | `0` | Which library grid to clue (`0` = highest quality) |
| `--keep-mean F` | `78` | **Quality floor.** Keep only grids whose mean answer-score ≥ F |
| `--max-iffy N` | `0` | **The key fill-quality lever** — see below |
| `--candidates N` | `200` | How many random grids to generate & screen |
| `--time SECS` | `2` | Per-grid fill budget |
| `--top N` | `20` | How many of the best grids to keep in the library |

### The quality levers (`--max-iffy`, `--keep-mean`)

Fill quality is the single biggest driver of how good the puzzle — and therefore
the clues and the QA verdict — turns out. Two flags control it:

- **`--max-iffy N`** — an "iffy" entry is one scoring **below 50** on the wordlist's
  1–100 scale (50 = a normal, fair answer; below 50 is questionable crosswordese,
  obscure abbreviations, awkward partials). `--max-iffy N` keeps only grids with
  **at most N** such entries.
  - **`--max-iffy 0` is the recommended default and the most important flag here.**
    It keeps only grids where *every* answer scores ≥ 50, which eliminates almost
    all `fill`-category QA findings **at the source** — before a single clue is
    written. If your QA reports keep flagging weak fill, this is the fix.
  - Trade-off: stricter gates reject more grids, so pair a strict `--max-iffy`
    with a higher `--candidates` (e.g. `--max-iffy 0 --candidates 400`) so enough
    grids survive. Themed grids are harder to fill cleanly, so you may need to
    relax to `--max-iffy 4`–`8` for themed mode.
- **`--keep-mean F`** — raises the *average* answer quality (not just the floor).
  78–80 yields polished, lively grids; lower it toward 70 if too few grids pass.

```bash
# Strict: only flawless-fill grids (screen more candidates to compensate)
./run-pipeline.sh --mode themeless --max-iffy 0 --keep-mean 80 --candidates 400
```

### Blocklist — banning specific words

Some junk entries are **mis-scored high** in the wordlist (e.g. `TOONIEBAR` is
scored 100), so no `--keep-mean`/`--max-iffy` setting can keep them out. For
those, edit **`fill-engine/data/blocklist.txt`** — one word per line; the engine
excludes them from every fill regardless of score:

```
# fill-engine/data/blocklist.txt
TOONIEBAR
EATSLIP
```

Case, spaces, and punctuation are ignored (`EAT SLIP` == `EATSLIP`). The engine
prints `blocklist: excluding N word(s)` on load. **Grow this list over time from
your QA findings** — it's the cheapest way to permanently retire words you never
want to see. Blocking a handful of words has no measurable effect on fill rate.

---

## Fixing QA findings

The script stops at QA. If the verdict isn't `ready`, fixing it depends on the
**category** of each finding — and the categories live at **two different layers**:

| QA category | Layer | Fix |
|---|---|---|
| `fill` | **Grid** — the answer itself is weak | Different grid or stricter gates |
| `duplicate` (same answer twice / near-dup) | **Grid** | Different grid |
| `duplicate` (answer appears in a clue) | **Clue** | `clue --revise` |
| `style`, `clue-accuracy`, `difficulty` | **Clue** | `clue --revise` |

**Fix grid-layer findings first** — a new grid changes the answers, which throws
away any clue work.

### Grid-level fixes (`fill`, duplicate-answers)

```bash
# Try the next-best grid in the same library, then re-clue + re-QA
./run-pipeline.sh --mode themeless --grid 1 --day Saturday

# …or regenerate the library so weak fill can't get in (then --grid 0)
./run-pipeline.sh --mode themeless --max-iffy 0 --keep-mean 80 --candidates 400
```

### Clue-level fixes (`style`, duplicate-in-clue, accuracy, difficulty)

Feed the QA report back; only the flagged clues are rewritten. Grid-level
findings it can't fix are reported as **unresolved** (go back to the step above).

```bash
cd clue-writer
npm run clue -- ../out/puzzles/themeless-grid0.clued.json \
               --revise ../out/puzzles/themeless-grid0.qa.json \
               --out ../out/puzzles/themeless-grid0.revised.json
npm run qa  -- ../out/puzzles/themeless-grid0.revised.json
```

Repeat **revise → qa** until the verdict is `ready` / `minor-revisions`.

---

## Theme ideation (optional, front of pipeline)

Stuck for a theme? Have Claude brainstorm sets that obey the grid constraints; it
prints ready-to-run commands you can hand to the themed pipeline.

```bash
cd clue-writer
npm run theme-idea -- --topic "hidden body parts" --count 3 --answers 3
# → pick a set, then:
cd .. && ./run-pipeline.sh --mode themed --themes <A>,<B>,<C>
```

---

## Multi-difficulty puzzles & post-solve explanations

The player can take a single puzzle that carries **three clue sets** (Easy /
Medium / Hard) plus an optional **one-line explanation** per answer, so the
solver picks a difficulty and gets a short "why this fits" note after solving.
Produce that bundle by running the clue writer once per tier on the **same
library grid**, then the explainer, then merge in the player's `import-puzzle`:

```bash
# 1. Build the grid library once (free)
./run-pipeline.sh --mode themeless --day Easy   # produces the library; clue+qa run for Easy too

# 2. Write the other two tiers against the SAME library grid
cd clue-writer
npm run clue -- ../out/libraries/grid-library.json --grid 0 --day Medium \
    --out ../out/puzzles/themeless-grid0.clued.medium.json
npm run clue -- ../out/libraries/grid-library.json --grid 0 --day Hard \
    --out ../out/puzzles/themeless-grid0.clued.hard.json

# 3. Post-solve explanations (run once; the answers are the same across tiers)
npm run explain -- ../out/puzzles/themeless-grid0.clued.json \
    --out ../out/puzzles/themeless-grid0.explained.json

# 4. Merge into one player puzzle
cd ../../xword-player
npm run import-puzzle -- \
  --easy   ../xword-pipeline/out/puzzles/themeless-grid0.clued.json \
  --medium ../xword-pipeline/out/puzzles/themeless-grid0.clued.medium.json \
  --hard   ../xword-pipeline/out/puzzles/themeless-grid0.clued.hard.json \
  --explanations ../xword-pipeline/out/puzzles/themeless-grid0.explained.json \
  --id mini-5 --title "Mini 5×5"
```

Cost: ~3× the clue step + 1 explain step per puzzle on Opus 4.7. The player
remains fully backward-compatible — puzzles imported the old single-tier way
(positional `<clued.json>`) still work; only the new bundles carry the extra
data that F4 (difficulty selector, graduated hints, post-solve explainers) will
spend.

## Running the tools individually

The script is just orchestration. Each tool is documented in its own README and
can be run directly:

- **fill-engine** (`library`, `theme`, `screen`, `xfill`): see
  [fill-engine/README.md](fill-engine/README.md). Run from `fill-engine/`.
- **clue-writer** (`clue`, `qa`, `theme-idea`, `clue --revise`): see
  [clue-writer/README.md](clue-writer/README.md). Run from `clue-writer/`.
  Default outputs go to `../out/…`; every command supports `--dry-run` to print
  its Claude prompt with no API call.

---

## Cost / notes

| Step | API? | Cost |
|---|---|---|
| `fill-engine` (grid generation) | No | free, seconds |
| `theme-idea` / `clue` / `qa` / `revise` | Yes | a few cents each (Opus 4.7) |

- **Themeless fills cleaner than themed.** Themed grids carry the theme answers
  plus equal-length mirror slots, so they're harder to fill — expect to relax
  `--max-iffy` and raise `--blocks`/`--candidates` for themed mode.
- Every Claude command has a `--dry-run` flag to preview the prompt for free.

---

## Contributing

PRs welcome — see [CONTRIBUTING.md](./CONTRIBUTING.md) for setup, the test/CI
flow, and conventions for fill-engine vs. clue-writer changes.

## License

This project is licensed under the [MIT License](./LICENSE).

The bundled wordlist at `fill-engine/data/xwordlist.dict` is distributed under
its own MIT license — see [`fill-engine/data/LICENSE-wordlist.txt`](./fill-engine/data/LICENSE-wordlist.txt)
for the original copyright and source attribution.
