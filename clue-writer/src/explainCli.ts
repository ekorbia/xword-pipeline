// CLI: write a one-sentence post-solve explanation for every answer in a clued
// puzzle. Output feeds the player via `import-puzzle --explanations`.
//
// Usage:
//   npm run explain -- <clued.json> [--out explained.json] [--dry-run]
//
// Requires ANTHROPIC_API_KEY (except with --dry-run).

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname } from "node:path";
import type { CluedPuzzle } from "./types.js";
import { buildExplainMessage, explainPuzzle } from "./explain.js";
import { dayLabel } from "./styleGuide.js";

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
  if (!args.input) throw new Error("usage: explain <clued.json> [--out explained.json] [--dry-run]");
  return args;
}

async function main() {
  const args = parseArgs(process.argv);
  const puzzle = JSON.parse(readFileSync(args.input, "utf8")) as CluedPuzzle;
  console.error(
    `explaining: ${args.input} | ${puzzle.themed ? "themed" : "themeless"} ${dayLabel(puzzle.day)} | ${puzzle.across.length + puzzle.down.length} entries`,
  );

  if (args.dryRun) {
    console.error("--- DRY RUN: explain prompt (no API call) ---\n");
    console.log(buildExplainMessage(puzzle));
    return;
  }
  if (!process.env.ANTHROPIC_API_KEY) {
    console.error("error: ANTHROPIC_API_KEY is not set. Export it, or use --dry-run.");
    process.exit(1);
  }

  const t0 = Date.now();
  const { items, warnings, usage } = await explainPuzzle(puzzle);
  const secs = ((Date.now() - t0) / 1000).toFixed(1);

  console.error(`\nexplained ${items.length} entries in ${secs}s  (cache read ${usage.cacheRead} tok)`);
  if (warnings.length) console.error(`warnings:\n  ${warnings.join("\n  ")}`);
  for (const it of items) console.error(`  ${it.num}${it.dir} (${it.answer}): ${it.explanation}`);

  const out = args.out || `${PUZZLES_DIR}/${basename(args.input).replace(/\.(clued\.[a-z]+|clued|json)$/i, "")}.explained.json`;
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, JSON.stringify({ source: basename(args.input), items }, null, 2));
  console.error(`\nwrote explanations -> ${out}`);
}

main().catch((err) => {
  console.error(err instanceof Error ? err.message : String(err));
  process.exit(1);
});
