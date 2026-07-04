# Changelog

## [0.1.0]

### Added
- **Expert clue tier** — clue writing now spans the full Easy → Expert range.
- **Duplicate-answer detection at two layers.** The Rust fill gate rejects any
  grid whose answers share a root — equal stems (`TEN`/`TENTH`, `EVEN`/`UNEVENLY`),
  one answer contained in another (`HOME`/`HOMERUN`), or a prominent shared
  embedded word (`BALL` in `BASEBALL`/`SEVEBALLESTEROS`) — so duplicates never
  reach the clue/QA steps. A deterministic clue checker then catches a grid answer
  appearing inside *another* entry's clue and auto-revises only the offending
  clues. Eliminates the most common high-severity QA finding; covered by tests in
  CI.
- **Interactive wizard** for `run-pipeline.sh` — prompts for mode, difficulty, and
  options instead of requiring flags.
- **Batch puzzle generation** via `generate-batch.sh` (many puzzles, varied
  sizes/days, in one command).
- **Detailed per-step pipeline timing** so you can see where wall-clock goes
  (fill vs. clue vs. QA).
- **Supplemental wordlist** support to broaden the fill engine's vocabulary.

### Changed
- **Size-aware difficulty defaults.** A bare themeless mini (≤7) now clues at
  Monday and a midi (8–12) at Wednesday; full-size themeless keeps the Saturday
  default (themed unchanged at Wednesday). The interactive wizard's suggested
  day follows the chosen size, and `run-pipeline.sh` prints a note when a ≤7
  mini is paired with a late-week `--day`.
- **Post-solve explanations default to Haiku 4.5** to cut cost — they're short and
  don't need a frontier model (override with `--explain-model`).
- **Themed generation is far more reliable.** Theme placement is now randomized
  across candidates (so their fills aren't correlated and fail together), per-mode
  defaults (gates / time / block count) are tuned separately for themed vs.
  themeless, the candidate screener early-stops once enough grids are kept, and a
  solver forward-check bug was corrected.
- **README:** added a link to the in-browser player at
  [wordfuzz.com/test](https://wordfuzz.com/test) — play puzzles you generate;
  files stay client-side.

### Fixed
- **QA judged minis against full-size day expectations.** The editorial reviewer
  now receives the same per-day rubric the clue writer wrote to plus a
  size-class rubric (mini ≤7 / midi 8–12 / full 13+), and every prompt (clue,
  QA, revise) states the grid's size — so a "Wednesday 5×5" is graded as a
  Wednesday-worded mini instead of a mid-week 15×15. Friday/Saturday guidance
  no longer asserts themeless-ness when the puzzle is themed.
- **Themed-generation deadlock.** Short or dense theme sets used to produce *zero*
  valid templates: a 2-cell vertical run trapped above a theme's bounding blocks
  could never be rescued one cell at a time. A seed-repair pass now blocks out
  doomed 1–2 cell runs (mirrored, to a fixpoint) so themed grids actually fill.
- **QA across multiple difficulty tiers** — the editorial pass now reviews each
  tier correctly instead of mishandling multi-tier puzzles.
