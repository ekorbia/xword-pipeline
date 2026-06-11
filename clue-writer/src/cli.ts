// CLI: write or revise crossword clues with Claude.
//
//   Write:   npm run clue -- <library.json> [--grid N] [--day D] [--out clued.json] [--dry-run]
//   Revise:  npm run clue -- <clued.json> --revise <qa.json> [--out clued.v2.json] [--dry-run]
//
// Requires ANTHROPIC_API_KEY (except with --dry-run).

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname } from "node:path";
import type { CluedPuzzle, Day, LibraryFile, QAReport } from "./types.js";
import { buildReviseMessage, buildUserMessage, reviseClues, writeClues } from "./clueWriter.js";
import { findAnswerInClueDups } from "./dupCheck.js";
import { defaultDay, difficultyWord, dayLabel, normalizeDay } from "./styleGuide.js";

const PUZZLES_DIR = "../out/puzzles";

/** Default output path under the shared out/ dir, derived from the input name. */
function outPathFor(explicit: string, inputPath: string, suffix: string): string {
  if (explicit) return explicit;
  return `${PUZZLES_DIR}/${basename(inputPath).replace(/\.json$/i, "")}${suffix}`;
}

function writeJson(path: string, data: unknown) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(data, null, 2));
}

function parseArgs(argv: string[]) {
  const args = { input: "", grid: 0, day: undefined as Day | undefined, out: "", dryRun: false, revise: "" };
  const rest = argv.slice(2);
  for (let i = 0; i < rest.length; i++) {
    const a = rest[i]!;
    if (a === "--grid") args.grid = Number(rest[++i]);
    else if (a === "--day") {
      const d = rest[++i]!;
      const match = normalizeDay(d);
      if (!match) {
        throw new Error(
          `bad --day '${d}' (use Monday..Saturday, or Easy/Medium/Tricky/Hard/Expert)`,
        );
      }
      args.day = match;
    } else if (a === "--out") args.out = rest[++i]!;
    else if (a === "--revise") args.revise = rest[++i]!;
    else if (a === "--dry-run") args.dryRun = true;
    else if (!args.input) args.input = a;
    else throw new Error(`unexpected argument: ${a}`);
  }
  if (!args.input) {
    throw new Error(
      "usage:\n  clue <library.json> [--grid N] [--day D] [--out clued.json] [--dry-run]\n  clue <clued.json> --revise <qa.json> [--out clued.v2.json] [--dry-run]",
    );
  }
  return args;
}

function requireKey() {
  if (!process.env.ANTHROPIC_API_KEY) {
    console.error("error: ANTHROPIC_API_KEY is not set. Export it, or use --dry-run.");
    process.exit(1);
  }
}

// ---- revise mode: rewrite only the clues the QA report flagged ----
async function runRevise(input: string, qaPath: string, out: string, dryRun: boolean) {
  const puzzle = JSON.parse(readFileSync(input, "utf8")) as CluedPuzzle;
  const report = JSON.parse(readFileSync(qaPath, "utf8")) as QAReport;
  console.error(
    `revising: ${input} | ${dayLabel(puzzle.day)} ${puzzle.themed ? "themed" : "themeless"} | report verdict: ${report.verdict}, ${report.findings.length} findings`,
  );

  if (dryRun) {
    console.error("--- DRY RUN: revise prompt (no API call) ---\n");
    console.log(buildReviseMessage(puzzle, report));
    return;
  }
  requireKey();

  const t0 = Date.now();
  const r = await reviseClues(puzzle, report);
  const secs = ((Date.now() - t0) / 1000).toFixed(1);

  console.error(`\nrewrote ${r.changed.length} clue(s) in ${secs}s (cache read ${r.usage.cacheRead} tok)\n`);
  for (const c of r.changed) {
    console.error(`  ${c.num}${c.dir} (${c.answer})  [fixes: ${c.addresses}]`);
    console.error(`    - ${c.before}`);
    console.error(`    + ${c.after}`);
  }
  if (r.unresolved.length) {
    console.error(`\nUNRESOLVED — these need a GRID change, not a re-clue (try another --grid or stricter library gates):`);
    for (const u of r.unresolved) console.error(`  · ${u.location}: ${u.reason}`);
  }
  if (r.warnings.length) console.error(`\nwarnings:\n  ${r.warnings.join("\n  ")}`);

  const revised: CluedPuzzle = {
    ...puzzle,
    difficulty: difficultyWord(puzzle.day),
    across: r.across,
    down: r.down,
  };
  const outPath = outPathFor(out, input, ".revised.json");
  writeJson(outPath, revised);
  console.error(`\nwrote revised puzzle -> ${outPath}`);
  console.error(`next: re-run QA →  npm run qa -- ${outPath}`);
}

// ---- write mode: clue a fresh grid from a library ----
async function runWrite(input: string, gridIdx: number, dayArg: Day | undefined, out: string, dryRun: boolean) {
  const lib = JSON.parse(readFileSync(input, "utf8")) as LibraryFile;
  const grid = lib.grids[gridIdx];
  if (!grid) throw new Error(`grid ${gridIdx} not found (library has ${lib.grids.length} grids)`);
  const day = dayArg ?? defaultDay(grid.themed);

  console.error(
    `library: ${input} | grid ${grid.id} (${grid.themed ? "themed" : "themeless"}, ${grid.entries.length} entries, mean ${grid.mean_score.toFixed(1)}) | day: ${dayLabel(day)}`,
  );

  if (dryRun) {
    console.error("--- DRY RUN: assembled user prompt (no API call) ---\n");
    console.log(buildUserMessage(grid, day));
    return;
  }
  requireKey();

  const t0 = Date.now();
  const result = await writeClues(grid, day);
  const secs = ((Date.now() - t0) / 1000).toFixed(1);

  const puzzle: CluedPuzzle = {
    day,
    difficulty: difficultyWord(day),
    themed: grid.themed,
    themes: lib.themes,
    blocks: grid.blocks,
    template: grid.template,
    fill: grid.fill,
    source: { wordlist: lib.wordlist, grid_id: grid.id },
    across: result.across,
    down: result.down,
  };

  console.error(
    `\nwrote ${result.across.length + result.down.length} clues in ${secs}s` +
      `  (in ${result.usage.input} / out ${result.usage.output} tok, cache read ${result.usage.cacheRead})`,
  );
  if (result.warnings.length) console.error(`warnings:\n  ${result.warnings.join("\n  ")}`);

  // Deterministic answer-in-clue dup check. Violations get ONE targeted
  // auto-revise pass (only the flagged clues are rewritten) before the puzzle
  // is written, so they never surface as high-severity QA findings.
  const dups = findAnswerInClueDups(puzzle.across, puzzle.down);
  if (dups.length > 0) {
    console.error(`\ndup check: ${dups.length} answer-in-clue violation(s) — auto-revising:`);
    for (const f of dups) console.error(`  · ${f.location}: ${f.issue}`);
    const report: QAReport = {
      verdict: "needs-work",
      summary: "Deterministic dup check: grid answers appear in clues.",
      findings: dups,
    };
    const rev = await reviseClues(puzzle, report);
    puzzle.across = rev.across;
    puzzle.down = rev.down;
    for (const c of rev.changed) {
      console.error(`  ${c.num}${c.dir} (${c.answer})\n    - ${c.before}\n    + ${c.after}`);
    }
    const left = findAnswerInClueDups(puzzle.across, puzzle.down);
    if (left.length > 0) {
      console.error(
        `warning: ${left.length} dup violation(s) remain after auto-revise (QA will flag them):\n  ${left.map((f) => `${f.location}: ${f.issue}`).join("\n  ")}`,
      );
    } else {
      console.error("dup check: clean after auto-revise");
    }
  }
  console.error("\nACROSS");
  for (const e of puzzle.across) console.error(`  ${e.num}. ${e.clue}   (${e.answer})`);
  console.error("\nDOWN");
  for (const e of puzzle.down) console.error(`  ${e.num}. ${e.clue}   (${e.answer})`);

  const outPath = outPathFor(out, input, `.clued.${day.toLowerCase()}.json`);
  writeJson(outPath, puzzle);
  console.error(`\nwrote clued puzzle -> ${outPath}`);
}

async function main() {
  const args = parseArgs(process.argv);
  if (args.revise) {
    await runRevise(args.input, args.revise, args.out, args.dryRun);
  } else {
    await runWrite(args.input, args.grid, args.day, args.out, args.dryRun);
  }
}

main().catch((err) => {
  console.error(err instanceof Error ? err.message : String(err));
  process.exit(1);
});
