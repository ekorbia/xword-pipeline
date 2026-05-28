// CLI: review a clued-puzzle JSON (output of the clue writer) and emit a QA report.
//
// Usage:
//   npm run qa -- <clued.json> [--out report.json] [--dry-run]
//
// Requires ANTHROPIC_API_KEY (except with --dry-run).

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname } from "node:path";
import type { CluedPuzzle } from "./types.js";
import { buildReviewMessage, reviewPuzzle } from "./editor.js";

const PUZZLES_DIR = "../out/puzzles";

function parseArgs(argv: string[]) {
  const args = { input: "", out: "", dryRun: false };
  const rest = argv.slice(2);
  for (let i = 0; i < rest.length; i++) {
    const a = rest[i]!;
    if (a === "--out") args.out = rest[++i]!;
    else if (a === "--dry-run") args.dryRun = true;
    else if (!args.input) args.input = a;
    else throw new Error(`unexpected argument: ${a}`);
  }
  if (!args.input) throw new Error("usage: qa <clued.json> [--out report.json] [--dry-run]");
  return args;
}

const SEV_ICON: Record<string, string> = { high: "!!", medium: " *", low: "  " };

async function main() {
  const args = parseArgs(process.argv);
  const puzzle = JSON.parse(readFileSync(args.input, "utf8")) as CluedPuzzle;
  console.error(
    `reviewing: ${args.input} | ${puzzle.themed ? "themed" : "themeless"} ${puzzle.day} | ${puzzle.across.length + puzzle.down.length} entries`,
  );

  if (args.dryRun) {
    console.error("--- DRY RUN: review prompt (no API call) ---\n");
    console.log(buildReviewMessage(puzzle));
    return;
  }
  if (!process.env.ANTHROPIC_API_KEY) {
    console.error("error: ANTHROPIC_API_KEY is not set. Export it, or use --dry-run.");
    process.exit(1);
  }

  const t0 = Date.now();
  const { report, usage } = await reviewPuzzle(puzzle);
  const secs = ((Date.now() - t0) / 1000).toFixed(1);

  const counts = { high: 0, medium: 0, low: 0 };
  for (const f of report.findings) counts[f.severity]++;

  console.error(`\nVERDICT: ${report.verdict.toUpperCase()}   (${counts.high} high, ${counts.medium} medium, ${counts.low} low)   ${secs}s, cache read ${usage.cacheRead} tok`);
  console.error(`\n${report.summary}\n`);
  for (const f of report.findings) {
    console.error(`${SEV_ICON[f.severity] ?? "  "} [${f.severity}/${f.category}] ${f.location}`);
    console.error(`     issue: ${f.issue}`);
    console.error(`     fix:   ${f.suggestion}`);
  }

  const out = args.out || `${PUZZLES_DIR}/${basename(args.input).replace(/\.json$/i, "")}.qa.json`;
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, JSON.stringify(report, null, 2));
  console.error(`\nwrote QA report -> ${out}`);
}

main().catch((err) => {
  console.error(err instanceof Error ? err.message : String(err));
  process.exit(1);
});
