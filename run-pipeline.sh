#!/usr/bin/env bash
#
# run-pipeline.sh — generate a crossword, clue it, and editorially review it.
# Runs the pipeline end-to-end THROUGH the QA step. Fixing/revising findings is
# a manual step afterward (see README.md → "Fixing QA findings").
#
#   ./run-pipeline.sh --mode themeless --blocks 36 --day Saturday
#   ./run-pipeline.sh --mode themeless --size 5 --day Easy        # 5x5 mini
#   ./run-pipeline.sh --mode themeless --tiers easy,medium,hard --explain
#       # ↑ multi-tier (3 clue sets) + post-solve explainer, one command
#   ./run-pipeline.sh --mode themed --themes WAITINGFORGODOT,ROCKETSCIENCE,TROMBONE
#
# All generated JSON lands under out/{libraries,puzzles}. Requires
# ANTHROPIC_API_KEY for the clue + qa steps (the grid step is free/offline).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
ENGINE="$ROOT/fill-engine"
CLUE="$ROOT/clue-writer"
OUT="$ROOT/out"

# ---- defaults ----
MODE="themeless"
SIZE=15            # grid dimension (themeless only; themed is 15-only for now)
BLOCKS=""          # default: ~16% of grid area themeless / 44 themed
CANDIDATES=200
TIME=2
KEEP_MEAN=78
MAX_IFFY=0
TOP=20
GRID=0
DAY=""             # empty → clue writer picks (Saturday themeless / Wednesday themed)
THEMES=""          # comma-separated, required for --mode themed
TIERS=""           # comma-separated subset of {easy,medium,hard}; empty = single-tier (today)
EXPLAIN=0          # 1 → also run the post-solve explainer
NO_QA=0            # 1 → skip the editorial QA step
SEED=1             # RNG seed for grid generation (bump to get a different library)

usage() {
  cat <<'EOF'
Usage: run-pipeline.sh [options]

  --mode themeless|themed   Pipeline mode (default: themeless)
  --size N                  Grid dimension, themeless only (default: 15; e.g. 5 or 10 for minis)
  --themes A,B,C            Theme answers (required for --mode themed; A-Z, no spaces)
  --blocks N                Black squares per grid (default: ~16% of area themeless / 44 themed)
  --candidates N            Random grids to generate & screen (default: 200)
  --time SECS               Per-grid fill budget (default: 2)
  --keep-mean F             Keep grids with mean answer-score >= F (default: 78)
  --max-iffy N              Keep grids with <= N entries scoring below 50 (default: 0)
  --top N                   Keep the best N grids in the library (default: 20)
  --grid N                  Which library grid to clue (default: 0 = best)
  --day DAY                 Monday..Saturday, or Easy/Medium/Tricky/Hard/Expert (default: mode-based)
  --tiers easy,medium,hard  Multi-tier: write a clue set per tier on the SAME grid (overrides --day).
                            QA + --explain run against the medium tier (or the first listed).
  --explain                 Also run the post-solve explainer (feeds the player as --explanations).
  --no-qa                   Skip the editorial QA step (saves a Claude call).
  -h, --help                Show this help

Output: out/libraries/<grid|theme>-library.json, out/puzzles/<name>.clued[.<tier>].json,
        out/puzzles/<name>.qa.json, out/puzzles/<name>.explained.json
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode) MODE="$2"; shift 2 ;;
    --size) SIZE="$2"; shift 2 ;;
    --themes) THEMES="$2"; shift 2 ;;
    --blocks) BLOCKS="$2"; shift 2 ;;
    --candidates) CANDIDATES="$2"; shift 2 ;;
    --time) TIME="$2"; shift 2 ;;
    --keep-mean) KEEP_MEAN="$2"; shift 2 ;;
    --max-iffy) MAX_IFFY="$2"; shift 2 ;;
    --top) TOP="$2"; shift 2 ;;
    --grid) GRID="$2"; shift 2 ;;
    --day) DAY="$2"; shift 2 ;;
    --tiers) TIERS="$2"; shift 2 ;;
    --explain) EXPLAIN=1; shift ;;
    --no-qa) NO_QA=1; shift ;;
    --seed) SEED="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage; exit 2 ;;
  esac
done

# ---- validate --tiers values up front (fail before any expensive step) ----
if [[ -n "$TIERS" ]]; then
  IFS=',' read -ra _TIERS_PRECHECK <<< "$TIERS"
  for tier in "${_TIERS_PRECHECK[@]}"; do
    case "$tier" in
      easy|medium|hard) ;;
      *) echo "error: --tiers values must be easy, medium, or hard (got '$tier')" >&2; exit 2 ;;
    esac
  done
fi

mkdir -p "$OUT/libraries" "$OUT/puzzles" "$OUT/themes"

# ---- build the engine if needed ----
if [[ ! -x "$ENGINE/target/release/library" || ! -x "$ENGINE/target/release/theme" ]]; then
  echo "==> building fill-engine (first run)…"
  (cd "$ENGINE" && cargo build --release)
fi

WORDLIST="$ENGINE/data/xwordlist.dict"
LIB=""
NAME=""

echo "==> generating grid library (mode: $MODE)…"
if [[ "$MODE" == "themeless" ]]; then
  : "${BLOCKS:=$(( SIZE * SIZE * 16 / 100 ))}"
  LIB="$OUT/libraries/grid-library.json"
  NAME="themeless-grid${GRID}"
  "$ENGINE/target/release/library" \
    --wordlist "$WORDLIST" --size "$SIZE" --blocks "$BLOCKS" --candidates "$CANDIDATES" --time "$TIME" \
    --keep-mean "$KEEP_MEAN" --max-iffy "$MAX_IFFY" --top "$TOP" --seed "$SEED" --out "$LIB"
elif [[ "$MODE" == "themed" ]]; then
  : "${BLOCKS:=44}"
  if [[ -z "$THEMES" ]]; then
    echo "error: --mode themed requires --themes A,B,C" >&2; exit 2
  fi
  if [[ "$SIZE" != "15" ]]; then
    echo "error: --size is themeless-only for now (themed grids are 15x15); drop --size or use --mode themeless." >&2
    exit 2
  fi
  LIB="$OUT/libraries/theme-library.json"
  NAME="themed-grid${GRID}"
  THEME_FLAGS=()
  IFS=',' read -ra _T <<< "$THEMES"
  for t in "${_T[@]}"; do THEME_FLAGS+=(--theme "$t"); done
  "$ENGINE/target/release/theme" \
    --wordlist "$WORDLIST" "${THEME_FLAGS[@]}" --blocks "$BLOCKS" --candidates "$CANDIDATES" \
    --time "$TIME" --keep-mean "$KEEP_MEAN" --max-iffy "$MAX_IFFY" --top "$TOP" --out "$LIB"
else
  echo "error: --mode must be 'themeless' or 'themed'" >&2; exit 2
fi

# ---- abort early if no grid filled cleanly ----
COUNT=$(grep -o '"count": *[0-9]*' "$LIB" | head -1 | grep -o '[0-9]*' || echo 0)
if [[ "${COUNT:-0}" -eq 0 ]]; then
  echo "" >&2
  echo "No grids passed the quality gates (mean>=$KEEP_MEAN, iffy<=$MAX_IFFY)." >&2
  echo "Try: lower --keep-mean, raise --max-iffy, raise --candidates, or change --blocks." >&2
  exit 1
fi
echo "    library: $LIB ($COUNT clean grids)"

# ---- Claude steps need a key ----
if [[ -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "" >&2
  echo "Grid library is ready, but ANTHROPIC_API_KEY is not set — skipping clue + qa." >&2
  echo "Export your key and re-run, or clue manually (see README)." >&2
  exit 1
fi

QA="$OUT/puzzles/${NAME}.qa.json"
EXPLAINED="$OUT/puzzles/${NAME}.explained.json"
PRIMARY_CLUED=""                 # the clued file QA + explain run against
CLUED_LINES=()                   # human summary lines
IMPORT_TIER_FLAGS=()             # `--easy/--medium/--hard FILE` flags for the import hint

# ---- clue writing: single-tier (default) or multi-tier (--tiers, parallel) ----
if [[ -n "$TIERS" ]]; then
  if [[ -n "$DAY" ]]; then
    echo "warning: --day is ignored when --tiers is set (per-tier day words derived: Easy/Medium/Hard)" >&2
  fi
  IFS=',' read -ra _TIERS_ARR <<< "$TIERS"
  # Launch all tier clue calls in parallel; cache writes/reads overlap (the SDK
  # cache_control points at the same STYLE_GUIDE so trailing calls usually hit
  # the cache once the first writes it).
  declare -a _TIER_PIDS=()
  declare -a _TIER_FILES=()
  declare -a _TIER_NAMES=()
  for tier in "${_TIERS_ARR[@]}"; do
    word="$(tr '[:lower:]' '[:upper:]' <<< "${tier:0:1}")${tier:1}"   # Easy / Medium / Hard
    tier_clued="$OUT/puzzles/${NAME}.clued.${tier}.json"
    echo "==> clueing tier: $word → $(basename "$tier_clued") [parallel]"
    ( cd "$CLUE" && npx --yes tsx src/cli.ts "$LIB" --grid "$GRID" --day "$word" --out "$tier_clued" ) &
    _TIER_PIDS+=("$!")
    _TIER_FILES+=("$tier_clued")
    _TIER_NAMES+=("$tier")
  done
  TIER_FAIL=0
  for pid in "${_TIER_PIDS[@]}"; do
    if ! wait "$pid"; then TIER_FAIL=1; fi
  done
  if [[ "$TIER_FAIL" -ne 0 ]]; then
    echo "error: one or more tier clue calls failed" >&2
    exit 1
  fi
  # Bookkeeping in declared order.
  for i in "${!_TIER_NAMES[@]}"; do
    tier="${_TIER_NAMES[$i]}"
    tier_clued="${_TIER_FILES[$i]}"
    CLUED_LINES+=("  clued [${tier}]: $tier_clued")
    IMPORT_TIER_FLAGS+=("--${tier}" "../xword-pipeline/out/puzzles/$(basename "$tier_clued")")
    # primary = medium if present, else the first listed tier
    if [[ "$tier" == "medium" || -z "$PRIMARY_CLUED" ]]; then PRIMARY_CLUED="$tier_clued"; fi
  done
else
  PRIMARY_CLUED="$OUT/puzzles/${NAME}.clued.json"
  echo "==> writing clues (Claude)…"
  DAY_ARG=()
  [[ -n "$DAY" ]] && DAY_ARG=(--day "$DAY")
  (cd "$CLUE" && npx --yes tsx src/cli.ts "$LIB" --grid "$GRID" "${DAY_ARG[@]}" --out "$PRIMARY_CLUED")
  CLUED_LINES+=("  puzzle  : $PRIMARY_CLUED")
fi

# ---- QA + explain (both depend only on PRIMARY_CLUED → run in parallel) ----
declare -a _POST_PIDS=()
declare -a _POST_LABELS=()
if [[ "$NO_QA" -eq 0 ]]; then
  echo "==> editorial QA (Claude) on $(basename "$PRIMARY_CLUED") [parallel]"
  ( cd "$CLUE" && npx --yes tsx src/qaCli.ts "$PRIMARY_CLUED" --out "$QA" ) &
  _POST_PIDS+=("$!"); _POST_LABELS+=("QA")
fi
if [[ "$EXPLAIN" -eq 1 ]]; then
  echo "==> post-solve explanations (Claude) on $(basename "$PRIMARY_CLUED") [parallel]"
  ( cd "$CLUE" && npx --yes tsx src/explainCli.ts "$PRIMARY_CLUED" --out "$EXPLAINED" ) &
  _POST_PIDS+=("$!"); _POST_LABELS+=("explain")
fi
POST_FAIL=0
for i in "${!_POST_PIDS[@]}"; do
  pid="${_POST_PIDS[$i]}"
  label="${_POST_LABELS[$i]}"
  if ! wait "$pid"; then echo "warning: $label step failed" >&2; POST_FAIL=1; fi
done
VERDICT=""
if [[ "$NO_QA" -eq 0 && -f "$QA" ]]; then
  VERDICT=$(grep -o '"verdict": *"[^"]*"' "$QA" | head -1 | sed 's/.*"\([^"]*\)"$/\1/')
fi

# ---- summary + suggested import command ----
echo ""
echo "================= pipeline complete ================="
echo "  library : $LIB"
for line in "${CLUED_LINES[@]}"; do echo "$line"; done
[[ "$NO_QA" -eq 0 ]] && echo "  QA      : $QA   (verdict: ${VERDICT:-unknown})"
[[ "$EXPLAIN" -eq 1 ]] && echo "  explain : $EXPLAINED"
echo "-----------------------------------------------------"
echo "  to import (run from xword-player):"
if [[ -n "$TIERS" ]]; then
  IMPORT_CMD=("npm" "run" "import-puzzle" "--" "${IMPORT_TIER_FLAGS[@]}")
else
  IMPORT_CMD=("npm" "run" "import-puzzle" "--" "../xword-pipeline/out/puzzles/$(basename "$PRIMARY_CLUED")")
fi
[[ "$EXPLAIN" -eq 1 ]] && IMPORT_CMD+=("--explanations" "../xword-pipeline/out/puzzles/$(basename "$EXPLAINED")")
echo "    ${IMPORT_CMD[*]}"
if [[ "$NO_QA" -eq 0 && "$VERDICT" != "ready" ]]; then
  echo ""
  echo "  Not 'ready'? Fix findings — see README.md → 'Fixing QA findings'."
  echo "  Clue-level:  (cd clue-writer && npm run clue -- $PRIMARY_CLUED --revise $QA)"
  echo "  Grid-level:  re-run with --grid $((GRID+1)), or stricter --keep-mean / --max-iffy 0."
fi
# Propagate any parallel-step failure as the script's exit code.
[[ "${POST_FAIL:-0}" -ne 0 ]] && exit 1
exit 0
