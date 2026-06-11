# clue-writer

The **Claude layer** of the crossword pipeline. Three tools, all built on the
official `@anthropic-ai/sdk` (per-step models live in `src/models.ts`:
Opus 4.7 for clue/QA, Sonnet 4.6 for theme ideation, Haiku 4.5 for the
explainer), adaptive thinking,
`effort: high`, prompt-cached system prompts, and structured (zod) output:

| Command | Stage | What it does |
|---|---|---|
| `theme-idea` | front | Propose theme sets (answers + revealer) → feed the xfill `theme` generator |
| `clue` | middle | Write one day-graded clue per answer for a filled grid |
| `qa` | back | Editorially review a finished puzzle and flag issues |

The pipeline: **`theme-idea` → (xfill `theme`/`library`) → `clue` → `qa` ⇄ `clue --revise`**.
Each tool consumes/produces JSON so stages compose, and the `qa`/`revise` pair
forms an iterate-until-clean editorial loop.

## Setup

```bash
npm install
export ANTHROPIC_API_KEY=sk-ant-...
```

All three commands support `--dry-run` (print the assembled prompt and exit, no
API key needed) and write a JSON artifact.

## `clue` — write clues

Paths are relative to `clue-writer/`. Libraries live in `../out/libraries/`;
outputs default to `../out/puzzles/`.

```bash
npm run clue -- ../out/libraries/grid-library.json --grid 0 --day Saturday
npm run clue -- ../out/libraries/theme-library.json --grid 0     # themed → Wednesday default
```

| Flag | Default | Description |
|------|---------|-------------|
| `<library.json>` | (required) | A fill-engine grid-library file (in `../out/libraries/`) |
| `--grid N` | `0` | Which grid in the library to clue |
| `--day D` | Saturday (themeless) / Wednesday (themed) | Difficulty. A day (Monday…Saturday) **or** a friendly word: `Easy`=Mon, `Medium`=Wed, `Tricky`=Thu, `Hard`=Fri, `Expert`=Sat |
| `--out PATH` | `../out/puzzles/<input>.clued.<day>.json` | Output clued-puzzle JSON |
| `--dry-run` | off | Print the assembled prompt and exit |

Writes a `CluedPuzzle` JSON (template, fill, and `across`/`down` arrays where each entry gains a `clue`). It records both `day` (the NYT-style calibration day, e.g. `"Saturday"`) and a friendly `difficulty` word (e.g. `"Expert"`).

> **Why keep the day-of-week internally?** The Mon→Sat day is the actual
> calibration signal the clue writer feeds Claude — the model has a precise,
> trained sense of how a "Tuesday clue" differs from a "Saturday clue," which a
> generic "Hard"/"Expert" wouldn't convey as well. So day-of-week stays as the
> internal difficulty model; the friendly word is for humans (and the play app's
> difficulty badge). The mapping: Easy=Mon/Tue, Medium=Wed, Tricky=Thu, Hard=Fri, Expert=Sat.

### Revise — fix clues flagged by QA

Feed a QA report back in to rewrite **only** the flagged clues (leaving good ones
untouched). Clue-level findings (style, duplicate-in-clue, accuracy, difficulty)
get rewritten; grid-level findings (weak `fill`, duplicate answers) are reported
as **`unresolved`** — those need a different grid, not a re-clue.

```bash
npm run clue -- ../out/puzzles/grid-library.clued.saturday.json \
               --revise ../out/puzzles/grid-library.clued.saturday.qa.json
```

| Flag | Description |
|------|-------------|
| `<clued.json>` | The clued puzzle to fix (output of `clue`) |
| `--revise <qa.json>` | The QA report (output of `qa`) |
| `--out PATH` | Revised puzzle (default `../out/puzzles/<input>.revised.json`) |
| `--dry-run` | Print the revise prompt and exit |

Prints a before/after diff of each changed clue and any unresolved (grid-level)
findings, then suggests re-running `qa`. This closes the loop into a proper
**clue → qa → revise → qa** cycle; iterate until the verdict is acceptable.

## `qa` — editorial review

Reviews a finished `CluedPuzzle` and reports severity-ranked findings (weak fill,
duplicates, clue-accuracy errors, unfair crossings, difficulty miscalibration,
breakfast-test, theme consistency, style) plus a verdict.

```bash
npm run qa -- ../out/puzzles/grid-library.clued.saturday.json
npm run qa -- examples/sample-clued.json --dry-run
```

| Flag | Default | Description |
|------|---------|-------------|
| `<clued.json>` | (required) | A clued-puzzle JSON (output of `clue`) |
| `--out PATH` | `../out/puzzles/<input>.qa.json` | Output QA report |
| `--dry-run` | off | Print the review prompt and exit |

## `explain` — post-solve explanations

Writes a one-sentence, post-solve explanation for every answer in a clued
puzzle — the fact, reference, or wordplay trick that makes each answer click.
Feeds the player (`import-puzzle --explanations`) for the post-solve experience.
Solve spoilers are fine here (the solver has already finished).

```bash
npm run explain -- ../out/puzzles/themeless-grid0.clued.json
npm run explain -- examples/sample-clued.json --dry-run
```

| Flag | Default | Description |
|------|---------|-------------|
| `<clued.json>` | (required) | A clued-puzzle JSON (output of `clue`) |
| `--out PATH` | `../out/puzzles/<input>.explained.json` | Output explanations file |
| `--model <id>` | `claude-haiku-4-5` | Model to use. Default is Haiku 4.5 — short factual recaps don't need Opus. Pass `claude-opus-4-7` to restore the prior behavior |
| `--dry-run` | off | Print the explain prompt and exit |

Output shape: `{ source, items: [{ num, dir, answer, explanation }, ...] }`.

## `theme-idea` — brainstorm themes

Proposes theme sets honoring the generator's grid constraints (1–4 answers, each
3–15 letters but never exactly 12), validates each answer client-side, and prints
a ready-to-run xfill `theme` command for each buildable set.

```bash
npm run theme-idea -- --topic "hidden body parts" --count 3 --answers 3 --out themes.json
npm run theme-idea -- --dry-run
```

| Flag | Default | Description |
|------|---------|-------------|
| `--topic "..."` | (none) | Optional theme seed/direction |
| `--count N` | `3` | Number of candidate theme sets |
| `--answers N` | `3` | Target theme answers per set |
| `--out PATH` | (none) | Optional: write ideas JSON |
| `--dry-run` | off | Print the ideation prompt and exit |

## How it works

- `styleGuide.ts` — the **cached system prompt**: inviolable cluing rules
  (no answer-in-clue, POS/tense agreement, abbreviation signals, `?` for
  wordplay, theme handling) plus a per-day difficulty rubric. Stable, so it
  prompt-caches across puzzles.
- `clueWriter.ts` — builds the per-puzzle user message (answers + theme flags +
  day guidance), calls `messages.parse` with a zod schema, and maps clues back
  to entries by number + direction. Includes a light fairness check that warns
  if a clue contains its own answer.
- `cli.ts` — file I/O and the `--dry-run` path.

## Notes

- Quality of the *fill* (and thus how cluable each entry is) comes from the
  Rust engine. Clue this from clean libraries (themeless grids run mean ~75–80
  with zero weak entries); rough themed fills will produce awkward clues for any
  glue entries.
- One Claude call per puzzle (all answers together) so the model can calibrate
  difficulty across the grid, keep theme clues consistent, and avoid repeating
  clue gimmicks.
