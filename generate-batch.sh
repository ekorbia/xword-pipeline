#!/usr/bin/env bash
#
# generate-batch.sh — produce N themeless puzzles in one orchestrated run by
# looping run-pipeline.sh, one puzzle per consecutive date. Each puzzle gets
# a unique --name (puzzle-YYYY-MM-DD), so output files never collide. Common
# use is a week of daily puzzles, but the pattern length is arbitrary (1..N).
#
# Quick start:
#   ./generate-batch.sh \
#     --start 2026-06-09 \
#     --pattern 10,10,10,15,15,15,15 \
#     --tiers easy,medium,hard
#
#     → 7 puzzles, dates 2026-06-09..2026-06-15, sizes per --pattern.
#     → defaults: QA OFF (cheaper), EXPLAIN ON, continue-on-fail.
#     → prints a per-puzzle status summary and writes an executable
#       import script at out/imports-<start>.sh.
#
# Run-pipeline.sh's own per-step output streams to the terminal so you can
# follow progress; nothing is hidden.
#
# Failure mode: by default an individual puzzle's failure is logged and the
# batch continues. Pass --abort-on-fail to bail on first error.
#
# QA is OFF by default in batch mode (per project preference). The fill
# engine already guarantees a solvable puzzle; QA only adds quality findings,
# which aren't acted on in batch context anyway. Opt in with --qa if you
# want verdict snapshots.

# Same rationale as run-pipeline.sh: bash 3.2's empty-array-under-set-u
# behavior causes silent subshell aborts; we stick to -eo and validate
# explicitly.
set -eo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
RUN_PIPELINE="$ROOT/run-pipeline.sh"
OUT_PUZZLES="$ROOT/out/puzzles"
OUT_DIR="$ROOT/out"

# ---- defaults ----
START=""
PATTERN=""
TIERS=""
QA=0                  # 0 = pass --no-qa to run-pipeline.sh (default cheap mode)
EXPLAIN=1             # 1 = pass --explain to run-pipeline.sh
EXPLAIN_MODEL=""
MAX_IFFY=""
KEEP_MEAN=""
CANDIDATES=300        # batch default; run-pipeline.sh's own default is 200, but batch favors a slightly deeper per-puzzle library (more clean grids kept). Override with --candidates.
ABORT_ON_FAIL=0
SEED=""               # base seed; auto-derived from $(date +%s) if unset. Each puzzle uses BASE_SEED + index, so seeds are distinct across the batch even if runs happen back-to-back.

usage() {
  cat <<'EOF'
Usage: generate-batch.sh --start YYYY-MM-DD --pattern N,N,... --tiers T,T,... [options]

Required:
  --start YYYY-MM-DD         First puzzle's date; subsequent puzzles increment by 1 day each.
  --pattern N,N,N,...        Comma-separated grid sizes; ONE entry per puzzle in the batch.
                             Number of entries determines the number of puzzles.
                             E.g. "10,10,10,15,15,15,15" makes 7 puzzles, three 10x10 then four 15x15.
  --tiers easy,medium[,hard,expert]
                             Tier set; uniform across the batch. Passed through to run-pipeline.sh.

Optional:
  --qa                       Run editorial QA on each puzzle (default: OFF in batch — cheaper).
  --no-explain               Skip the post-solve explainer (default: ON).
  --explain-model <id>       Override the explainer's model (default: claude-haiku-4-5).
  --max-iffy N               Pass-through fill-quality lever.
  --keep-mean F              Pass-through fill-quality lever.
  --candidates N             Fill candidate count per puzzle (batch default: 300; run-pipeline.sh default: 200).
  --seed N                   Base seed for this batch (each puzzle uses BASE_SEED + index).
                             Defaults to current epoch — different invocations produce different
                             puzzles. Pass an explicit N to reproduce a specific batch.
  --abort-on-fail            Stop on first per-puzzle failure (default: log + continue).
  -h, --help                 Show this help.

Output:
  - Per-puzzle files at out/puzzles/puzzle-YYYY-MM-DD.{clued.<tier>,qa.<tier>,explained}.json
  - Generated import script at out/imports-<start>.sh (executable; one-command import).
  - Status table + import-commands block to stdout at the end.
EOF
}

# ---- parse args ----
while [[ $# -gt 0 ]]; do
  case "$1" in
    --start) START="$2"; shift 2 ;;
    --pattern) PATTERN="$2"; shift 2 ;;
    --tiers) TIERS="$2"; shift 2 ;;
    --qa) QA=1; shift ;;
    --no-explain) EXPLAIN=0; shift ;;
    --explain-model) EXPLAIN_MODEL="$2"; shift 2 ;;
    --max-iffy) MAX_IFFY="$2"; shift 2 ;;
    --keep-mean) KEEP_MEAN="$2"; shift 2 ;;
    --candidates) CANDIDATES="$2"; shift 2 ;;
    --seed) SEED="$2"; shift 2 ;;
    --abort-on-fail) ABORT_ON_FAIL=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage; exit 2 ;;
  esac
done

# ---- validate required inputs ----
[[ -n "$START"   ]] || { echo "error: --start is required" >&2; usage; exit 2; }
[[ -n "$PATTERN" ]] || { echo "error: --pattern is required" >&2; usage; exit 2; }
[[ -n "$TIERS"   ]] || { echo "error: --tiers is required" >&2; usage; exit 2; }
[[ -x "$RUN_PIPELINE" ]] || { echo "error: $RUN_PIPELINE not found or not executable" >&2; exit 2; }

if [[ ! "$START" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
  echo "error: --start must be YYYY-MM-DD (got '$START')" >&2; exit 2
fi

# Validate --pattern is comma-separated positive integers (no size restriction).
IFS=',' read -ra _SIZES <<< "$PATTERN"
[[ "${#_SIZES[@]}" -gt 0 ]] || { echo "error: --pattern must list at least one size" >&2; exit 2; }
for s in "${_SIZES[@]}"; do
  if [[ ! "$s" =~ ^[0-9]+$ ]] || [[ "$s" -lt 1 ]]; then
    echo "error: --pattern value '$s' is not a positive integer" >&2; exit 2
  fi
done

# Validate --tiers values (mirrors run-pipeline.sh).
IFS=',' read -ra _TIERS_PRECHECK <<< "$TIERS"
for tier in "${_TIERS_PRECHECK[@]}"; do
  case "$tier" in
    easy|medium|hard|expert) ;;
    *) echo "error: --tiers value must be easy/medium/hard/expert (got '$tier')" >&2; exit 2 ;;
  esac
done

# ---- date arithmetic: BSD (macOS) vs GNU ----
add_days() {
  local d="$1" n="$2"
  if date -j -f "%Y-%m-%d" "$d" +%s > /dev/null 2>&1; then
    date -j -v"+${n}d" -f "%Y-%m-%d" "$d" "+%Y-%m-%d"     # BSD/macOS
  else
    date -d "$d + $n days" "+%Y-%m-%d"                     # GNU/Linux
  fi
}

# ---- helpers ----
# Pull `verdict` field out of a QA JSON file; returns empty if file missing/malformed.
extract_verdict() {
  local f="$1"
  [[ -f "$f" ]] || { echo ""; return; }
  grep -o '"verdict": *"[^"]*"' "$f" | head -1 | sed 's/.*"\([^"]*\)"$/\1/'
}

# Build the worst-case verdict across an array of QA files (for the summary line).
qa_rank() { case "$1" in ready) echo 0;; minor-revisions|revise_clues) echo 1;; needs-work|regenerate_grid) echo 2;; *) echo 1;; esac; }
worst_verdict_of() {
  local worst="ready" v
  for f in "$@"; do
    v=$(extract_verdict "$f")
    [[ -z "$v" ]] && continue
    (( $(qa_rank "$v") > $(qa_rank "$worst") )) && worst="$v"
  done
  echo "$worst"
}

# ---- run the batch ----
NUM_PUZZLES="${#_SIZES[@]}"
WEEK_START_T0=$SECONDS
# Base seed for the batch. Each puzzle uses BASE_SEED + index so seeds are
# distinct even if back-to-back run-pipeline.sh invocations would otherwise
# share a wall-clock second. Recording this in the summary lets you reproduce
# the exact batch later via `--seed $BASE_SEED`.
BASE_SEED="${SEED:-$(date +%s)}"

echo ""
echo "================= generate-batch ================="
echo "  Start       : $START"
echo "  Puzzles     : $NUM_PUZZLES   (sizes: $PATTERN)"
echo "  Tiers       : $TIERS"
echo "  QA          : $([[ $QA -eq 1 ]] && echo on || echo off)"
echo "  Explain     : $([[ $EXPLAIN -eq 1 ]] && echo "on (${EXPLAIN_MODEL:-claude-haiku-4-5})" || echo off)"
echo "  Base seed   : $BASE_SEED   (reproduce this batch: --seed $BASE_SEED)"
echo "  Abort       : $([[ $ABORT_ON_FAIL -eq 1 ]] && echo "on first failure" || echo "continue on failure")"
echo "================================================="
echo ""

# Per-puzzle bookkeeping. Parallel arrays keyed by index.
declare -a STATUS=()         # "ok" | "fail:<stage>"
declare -a DATES=()          # YYYY-MM-DD
declare -a SIZES=()          # grid dim
declare -a NAMES=()          # puzzle-YYYY-MM-DD
declare -a VERDICTS=()       # worst QA verdict per puzzle (empty if QA off)

for (( i=0; i<NUM_PUZZLES; i++ )); do
  date=$(add_days "$START" "$i")
  size="${_SIZES[$i]}"
  name="puzzle-$date"
  DATES+=("$date")
  SIZES+=("$size")
  NAMES+=("$name")

  echo ""
  echo "================= puzzle $((i+1))/$NUM_PUZZLES — $date (${size}x${size}) ================="

  # Build args for run-pipeline.sh
  puzzle_seed=$((BASE_SEED + i))
  args=(--mode themeless --size "$size" --tiers "$TIERS" --name "$name" --date "$date" --seed "$puzzle_seed")
  [[ "$QA" -eq 0 ]] && args+=(--no-qa)
  [[ "$EXPLAIN" -eq 1 ]] && args+=(--explain)
  [[ -n "$EXPLAIN_MODEL" ]] && args+=(--explain-model "$EXPLAIN_MODEL")
  [[ -n "$MAX_IFFY" ]] && args+=(--max-iffy "$MAX_IFFY")
  [[ -n "$KEEP_MEAN" ]] && args+=(--keep-mean "$KEEP_MEAN")
  [[ -n "$CANDIDATES" ]] && args+=(--candidates "$CANDIDATES")

  # Invoke run-pipeline.sh. Capture exit code without tripping set -e.
  rc=0
  "$RUN_PIPELINE" "${args[@]}" || rc=$?

  if [[ "$rc" -eq 0 ]]; then
    STATUS+=("ok")
    if [[ "$QA" -eq 1 ]]; then
      # Collect per-tier QA files for this puzzle and derive the worst verdict.
      qa_files=()
      IFS=',' read -ra _T_LOCAL <<< "$TIERS"
      for t in "${_T_LOCAL[@]}"; do
        qa_files+=("$OUT_PUZZLES/${name}.qa.${t}.json")
      done
      VERDICTS+=("$(worst_verdict_of "${qa_files[@]}")")
    else
      VERDICTS+=("")
    fi
  else
    STATUS+=("fail:rc=$rc")
    VERDICTS+=("")
    echo "warning: puzzle $date (${size}x${size}) failed (run-pipeline.sh exit $rc)" >&2
    if [[ "$ABORT_ON_FAIL" -eq 1 ]]; then
      echo "error: --abort-on-fail was set; stopping after first failure" >&2
      break
    fi
  fi
done

WEEK_WALL=$((SECONDS - WEEK_START_T0))

# ---- summary ----
ok_count=0
for s in "${STATUS[@]}"; do [[ "$s" == "ok" ]] && (( ok_count++ )); done

echo ""
echo "================= batch complete ================="
for (( i=0; i<${#STATUS[@]}; i++ )); do
  mark="✓"; [[ "${STATUS[$i]}" != "ok" ]] && mark="✗"
  base=$(printf "  %s  %s (%sx%s)" "$mark" "${DATES[$i]}" "${SIZES[$i]}" "${SIZES[$i]}")
  if [[ "${STATUS[$i]}" == "ok" ]]; then
    if [[ -n "${VERDICTS[$i]}" ]]; then
      echo "$base   ${VERDICTS[$i]}"
    else
      echo "$base   ok"
    fi
  else
    echo "$base   FAILED (${STATUS[$i]})"
  fi
done
echo ""
echo "  Successful : ${ok_count}/${#STATUS[@]}"
printf "  Wall-clock : %dm %02ds\n" "$((WEEK_WALL / 60))" "$((WEEK_WALL % 60))"
echo "  Base seed  : $BASE_SEED   (reproduce: --seed $BASE_SEED)"
echo "================================================="

# ---- generate executable import script for one-command import ----
IMPORTS_SCRIPT="$OUT_DIR/imports-${START}.sh"
{
  cat <<EOF
#!/usr/bin/env bash
# Auto-generated by generate-batch.sh for batch starting $START.
# Imports all successful puzzles into the player in one shot.
#
# Run from anywhere:
#   bash $IMPORTS_SCRIPT
#
# Assumes xword-player is a sibling directory of xword-pipeline. If yours
# isn't, set XWORD_PLAYER explicitly:
#   XWORD_PLAYER=/path/to/xword-player bash $IMPORTS_SCRIPT
set -eo pipefail

ROOT="\$(cd "\$(dirname "\$0")/.." && pwd)"   # xword-pipeline/
XWORD_PLAYER="\${XWORD_PLAYER:-\$(cd "\$ROOT/../xword-player" 2>/dev/null && pwd || true)}"
if [[ -z "\$XWORD_PLAYER" || ! -d "\$XWORD_PLAYER" ]]; then
  echo "error: xword-player not found; set XWORD_PLAYER=/path/to/player" >&2; exit 1
fi
cd "\$XWORD_PLAYER"

PUZZLES="\$ROOT/out/puzzles"

EOF
  IFS=',' read -ra _T_IMP <<< "$TIERS"
  for (( i=0; i<${#STATUS[@]}; i++ )); do
    [[ "${STATUS[$i]}" != "ok" ]] && continue
    d="${DATES[$i]}"
    n="${NAMES[$i]}"
    sz="${SIZES[$i]}"
    echo ""
    echo "echo \"==> import $d (${sz}x${sz})\""
    echo "npm run import-puzzle -- \\"
    for t in "${_T_IMP[@]}"; do
      echo "  --$t \"\$PUZZLES/${n}.clued.${t}.json\" \\"
    done
    [[ "$EXPLAIN" -eq 1 ]] && echo "  --explanations \"\$PUZZLES/${n}.explained.json\" \\"
    echo "  --date $d"
  done
} > "$IMPORTS_SCRIPT"
chmod +x "$IMPORTS_SCRIPT"

# ---- import commands block to stdout ----
echo ""
if [[ "$ok_count" -gt 0 ]]; then
  echo "  To import (run from xword-player):"
  echo ""
  IFS=',' read -ra _T_IMP <<< "$TIERS"
  for (( i=0; i<${#STATUS[@]}; i++ )); do
    [[ "${STATUS[$i]}" != "ok" ]] && continue
    d="${DATES[$i]}"
    n="${NAMES[$i]}"
    line="    npm run import-puzzle --"
    for t in "${_T_IMP[@]}"; do
      line+=" --$t ../xword-pipeline/out/puzzles/${n}.clued.${t}.json"
    done
    [[ "$EXPLAIN" -eq 1 ]] && line+=" --explanations ../xword-pipeline/out/puzzles/${n}.explained.json"
    line+=" --date $d"
    echo "$line"
  done
  echo ""
  echo "  Or import all in one shot:"
  echo "    bash $IMPORTS_SCRIPT"
fi
echo ""

# Exit code: 0 if at least one puzzle succeeded, else 1.
[[ "$ok_count" -gt 0 ]] || exit 1
exit 0
