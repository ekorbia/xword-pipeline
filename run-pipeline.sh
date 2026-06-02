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

# Per-task timings (each background subshell writes its elapsed seconds here on
# EXIT; the orchestrator reads them at the end to render the timing summary).
# Cleaned up unconditionally on script exit.
TIMING_DIR=$(mktemp -d)
trap 'rm -rf "$TIMING_DIR"' EXIT

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
EXPLAIN_MODEL=""   # override model for the explain pass (default: explainer's own default — Haiku 4.5)
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
  --tiers easy,medium,hard[,expert]
                            Multi-tier: write a clue set per tier on the SAME grid (overrides --day).
                            Each tier is day-calibrated (easy=Monday, medium=Wednesday,
                            hard=Friday, expert=Saturday) and QA runs against each tier
                            independently. --explain still runs once against the medium
                            (or first listed) tier — explanations are tier-agnostic.
  --explain                 Also run the post-solve explainer (feeds the player as --explanations).
  --explain-model <id>      Override the explainer's model (default: claude-haiku-4-5).
                            Pass claude-opus-4-7 to restore the prior, higher-cost behavior.
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
    --explain-model) EXPLAIN_MODEL="$2"; shift 2 ;;
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
      easy|medium|hard|expert) ;;
      *) echo "error: --tiers values must be easy, medium, hard, or expert (got '$tier')" >&2; exit 2 ;;
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

T_FILL_START=$SECONDS
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
T_FILL=$((SECONDS - T_FILL_START))

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

EXPLAINED="$OUT/puzzles/${NAME}.explained.json"
PRIMARY_CLUED=""                 # the clued file explain runs against (medium / first listed)
CLUED_LINES=()                   # human summary lines
IMPORT_TIER_FLAGS=()             # `--easy/--medium/--hard/--expert FILE` flags for the import hint
QA_TARGETS=()                    # (label, clued, qaOut) triples flattened: 3 entries per QA job
QA_FILE_PRIMARY="$OUT/puzzles/${NAME}.qa.json"   # single-tier path (kept for compat)

# ---- clue writing: single-tier (default) or multi-tier (--tiers, parallel) ----
T_CLUE_START=$SECONDS
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
    (
      T0=$SECONDS
      trap 'echo $((SECONDS - T0)) > "$TIMING_DIR/clue-'"$tier"'.t"' EXIT
      cd "$CLUE" && npx --yes tsx src/cli.ts "$LIB" --grid "$GRID" --day "$word" --out "$tier_clued"
    ) &
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
T_CLUE=$((SECONDS - T_CLUE_START))

# ---- QA + explain (run in parallel) ----
# QA: in multi-tier mode, review EVERY tier file (each carries its own day
# calibration in the JSON), not just the primary. In single-tier mode, QA the
# only clued file.
# Explain: always runs once against PRIMARY_CLUED — explanations are
# tier-agnostic (they explain why the answer fits, not why the clue is hard).
T_POST_START=$SECONDS
declare -a _POST_PIDS=()
declare -a _POST_LABELS=()
declare -a _QA_OUTPUTS=()        # parallel arrays for per-tier verdict summary
declare -a _QA_LABELS=()
if [[ "$NO_QA" -eq 0 ]]; then
  if [[ -n "$TIERS" ]]; then
    for i in "${!_TIER_NAMES[@]}"; do
      tier="${_TIER_NAMES[$i]}"
      tier_clued="${_TIER_FILES[$i]}"
      tier_qa="$OUT/puzzles/${NAME}.qa.${tier}.json"
      echo "==> editorial QA (Claude) [${tier}] on $(basename "$tier_clued") [parallel]"
      (
        T0=$SECONDS
        trap 'echo $((SECONDS - T0)) > "$TIMING_DIR/qa-'"$tier"'.t"' EXIT
        cd "$CLUE" && npx --yes tsx src/qaCli.ts "$tier_clued" --out "$tier_qa"
      ) &
      _POST_PIDS+=("$!"); _POST_LABELS+=("QA[${tier}]")
      _QA_OUTPUTS+=("$tier_qa"); _QA_LABELS+=("$tier")
    done
  else
    echo "==> editorial QA (Claude) on $(basename "$PRIMARY_CLUED") [parallel]"
    (
      T0=$SECONDS
      trap 'echo $((SECONDS - T0)) > "$TIMING_DIR/qa-primary.t"' EXIT
      cd "$CLUE" && npx --yes tsx src/qaCli.ts "$PRIMARY_CLUED" --out "$QA_FILE_PRIMARY"
    ) &
    _POST_PIDS+=("$!"); _POST_LABELS+=("QA")
    _QA_OUTPUTS+=("$QA_FILE_PRIMARY"); _QA_LABELS+=("primary")
  fi
fi
if [[ "$EXPLAIN" -eq 1 ]]; then
  EXPLAIN_MODEL_ARGS=()
  [[ -n "$EXPLAIN_MODEL" ]] && EXPLAIN_MODEL_ARGS=(--model "$EXPLAIN_MODEL")
  echo "==> post-solve explanations (Claude) on $(basename "$PRIMARY_CLUED") [parallel]"
  (
    T0=$SECONDS
    trap 'echo $((SECONDS - T0)) > "$TIMING_DIR/explain.t"' EXIT
    cd "$CLUE" && npx --yes tsx src/explainCli.ts "$PRIMARY_CLUED" "${EXPLAIN_MODEL_ARGS[@]}" --out "$EXPLAINED"
  ) &
  _POST_PIDS+=("$!"); _POST_LABELS+=("explain")
fi
POST_FAIL=0
for i in "${!_POST_PIDS[@]}"; do
  pid="${_POST_PIDS[$i]}"
  label="${_POST_LABELS[$i]}"
  if ! wait "$pid"; then echo "warning: $label step failed" >&2; POST_FAIL=1; fi
done
T_POST=$((SECONDS - T_POST_START))

# Read every QA verdict (if QA ran). Track "worst" — any non-ready verdict
# is surfaced in the summary as a hint to revise.
declare -a _QA_VERDICTS=()
WORST_VERDICT="ready"     # ready < revise_clues < regenerate_grid (loose ordering)
qa_rank() { case "$1" in ready) echo 0;; revise_clues) echo 1;; regenerate_grid) echo 2;; *) echo 1;; esac; }
if [[ "$NO_QA" -eq 0 ]]; then
  for i in "${!_QA_OUTPUTS[@]}"; do
    qf="${_QA_OUTPUTS[$i]}"
    v=""
    if [[ -f "$qf" ]]; then
      v=$(grep -o '"verdict": *"[^"]*"' "$qf" | head -1 | sed 's/.*"\([^"]*\)"$/\1/')
    fi
    _QA_VERDICTS+=("${v:-unknown}")
    if [[ -n "$v" ]] && (( $(qa_rank "$v") > $(qa_rank "$WORST_VERDICT") )); then WORST_VERDICT="$v"; fi
  done
fi

# ---- summary + suggested import command ----
echo ""
echo "================= pipeline complete ================="
echo "  library : $LIB"
for line in "${CLUED_LINES[@]}"; do echo "$line"; done
if [[ "$NO_QA" -eq 0 ]]; then
  if [[ -n "$TIERS" ]]; then
    for i in "${!_QA_OUTPUTS[@]}"; do
      echo "  QA[${_QA_LABELS[$i]}] : ${_QA_OUTPUTS[$i]}   (verdict: ${_QA_VERDICTS[$i]})"
    done
  else
    echo "  QA      : ${_QA_OUTPUTS[0]}   (verdict: ${_QA_VERDICTS[0]:-unknown})"
  fi
fi
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
if [[ "$NO_QA" -eq 0 && "$WORST_VERDICT" != "ready" ]]; then
  echo ""
  echo "  Not all tiers 'ready'? Fix findings — see README.md → 'Fixing QA findings'."
  if [[ -n "$TIERS" ]]; then
    echo "  Each per-tier QA file pairs 1:1 with its clued.<tier>.json — revise that tier's clues:"
    for i in "${!_QA_LABELS[@]}"; do
      if [[ "${_QA_VERDICTS[$i]}" != "ready" ]]; then
        tier="${_QA_LABELS[$i]}"
        tier_clued="${_TIER_FILES[$i]}"
        tier_qa="${_QA_OUTPUTS[$i]}"
        echo "    [$tier] (cd clue-writer && npm run clue -- $tier_clued --revise $tier_qa)"
      fi
    done
  else
    echo "  Clue-level:  (cd clue-writer && npm run clue -- $PRIMARY_CLUED --revise ${_QA_OUTPUTS[0]})"
  fi
  echo "  Grid-level:  re-run with --grid $((GRID+1)), or stricter --keep-mean / --max-iffy 0."
fi

# ---- timing summary ----
# Stage lines show each stage's own wall-clock. When QA and Explain both ran,
# the synthetic "QA ‖ Explain" line shows the wall-clock cost they actually
# added to the total (= max of the two), so the reader can verify
# Fill + Clue + (QA‖Explain) ≈ Total. Per-tier sub-detail comes from each
# background subshell's EXIT trap writing its elapsed seconds to TIMING_DIR.
_read_time() { local f="$TIMING_DIR/$1.t"; [[ -f "$f" ]] && cat "$f" || echo "0"; }
echo ""
echo "================= timing summary ================="
printf "  Fill          : %3ds\n" "${T_FILL:-0}"
if [[ -n "$TIERS" ]]; then
  _BD=""
  for tier in "${_TIER_NAMES[@]}"; do
    _t=$(_read_time "clue-$tier")
    _BD+="${_BD:+ · }${tier} ${_t}s"
  done
  printf "  Clue          : %3ds  (%d tiers, parallel)\n" "${T_CLUE:-0}" "${#_TIER_NAMES[@]}"
  echo  "                  └ ${_BD}"
else
  printf "  Clue          : %3ds\n" "${T_CLUE:-0}"
fi
if [[ "$NO_QA" -eq 0 ]]; then
  _QA_MAX=0; _BD=""
  for label in "${_QA_LABELS[@]}"; do
    _t=$(_read_time "qa-$label")
    [[ "$_t" =~ ^[0-9]+$ ]] && (( _t > _QA_MAX )) && _QA_MAX=$_t
    _BD+="${_BD:+ · }${label} ${_t}s"
  done
  if [[ -n "$TIERS" ]]; then
    printf "  QA            : %3ds  (%d tiers, parallel)\n" "$_QA_MAX" "${#_QA_LABELS[@]}"
    echo  "                  └ ${_BD}"
  else
    printf "  QA            : %3ds\n" "$_QA_MAX"
  fi
fi
if [[ "$EXPLAIN" -eq 1 ]]; then
  _T_EXP=$(_read_time "explain")
  printf "  Explain       : %3ds  (%s)\n" "$_T_EXP" "${EXPLAIN_MODEL:-claude-haiku-4-5}"
fi
if [[ "$NO_QA" -eq 0 && "$EXPLAIN" -eq 1 ]]; then
  printf "  QA ‖ Explain  : %3ds  (these two ran in parallel; max governs)\n" "${T_POST:-0}"
fi
echo "  ──────────"
printf "  Total         : %3ds  (wall-clock)\n" "$SECONDS"
echo "==================================================="

# Propagate any parallel-step failure as the script's exit code.
[[ "${POST_FAIL:-0}" -ne 0 ]] && exit 1
exit 0
