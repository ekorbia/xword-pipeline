// CLI: export a clued puzzle to distribution formats (.puz / .ipuz / PDF).
//
//   npm run export -- <clued.json> [--format puz|ipuz|pdf|all]
//                                  [--out path] [--title T] [--author A]
//                                  [--copyright C] [--notes N] [--no-solution]
//
// Pure local conversion — no API key needed. Defaults to all three formats,
// written next to the input file with the matching extension.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import type { CluedPuzzle } from "./types.js";
import { resolveMeta, type ExportMeta } from "./exportShared.js";
import { toPuz } from "./puz.js";
import { toIpuz } from "./ipuz.js";
import { toPdf } from "./pdf.js";

type Format = "puz" | "ipuz" | "pdf";
const ALL_FORMATS: Format[] = ["puz", "ipuz", "pdf"];
const EXT: Record<Format, string> = { puz: ".puz", ipuz: ".ipuz", pdf: ".pdf" };

interface Args {
  input: string;
  formats: Format[];
  out: string; // explicit output path/base, or ""
  meta: Partial<ExportMeta>;
  solution: boolean;
}

function parseArgs(argv: string[]): Args {
  const args: Args = { input: "", formats: ALL_FORMATS, out: "", meta: {}, solution: true };
  const rest = argv.slice(2);
  for (let i = 0; i < rest.length; i++) {
    const a = rest[i]!;
    if (a === "--format") {
      const f = rest[++i]!;
      if (f === "all") args.formats = ALL_FORMATS;
      else if (f === "puz" || f === "ipuz" || f === "pdf") args.formats = [f];
      else throw new Error(`bad --format '${f}' (use puz, ipuz, pdf, or all)`);
    } else if (a === "--out") args.out = rest[++i]!;
    else if (a === "--title") args.meta.title = rest[++i]!;
    else if (a === "--author") args.meta.author = rest[++i]!;
    else if (a === "--copyright") args.meta.copyright = rest[++i]!;
    else if (a === "--notes") args.meta.notes = rest[++i]!;
    else if (a === "--no-solution") args.solution = false;
    else if (!args.input) args.input = a;
    else throw new Error(`unexpected argument: ${a}`);
  }
  if (!args.input) {
    throw new Error(
      "usage:\n  export <clued.json> [--format puz|ipuz|pdf|all] [--out path]\n" +
        "    [--title T] [--author A] [--copyright C] [--notes N] [--no-solution]",
    );
  }
  return args;
}

/** Output path for a format: explicit --out (used as a base for multi-format),
 * otherwise the input path with its extension swapped. */
function outPathFor(args: Args, fmt: Format): string {
  const base = (args.out || args.input).replace(/\.(json|puz|ipuz|pdf)$/i, "");
  return base + EXT[fmt];
}

function render(fmt: Format, puzzle: CluedPuzzle, meta: ExportMeta, solution: boolean): Buffer | string {
  switch (fmt) {
    case "puz":
      return toPuz(puzzle, meta);
    case "ipuz":
      return toIpuz(puzzle, meta);
    case "pdf":
      return toPdf(puzzle, meta, { solution });
  }
}

function main() {
  const args = parseArgs(process.argv);
  const puzzle = JSON.parse(readFileSync(args.input, "utf8")) as CluedPuzzle;
  const meta = resolveMeta(puzzle, args.meta);

  const counts = `${puzzle.across.length}A / ${puzzle.down.length}D`;
  console.error(
    `export: ${args.input} | ${puzzle.difficulty ?? puzzle.day} ${puzzle.themed ? "themed" : "themeless"} | ${counts}`,
  );

  for (const fmt of args.formats) {
    const data = render(fmt, puzzle, meta, args.solution);
    const path = outPathFor(args, fmt);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, data);
    const size = typeof data === "string" ? Buffer.byteLength(data) : data.length;
    console.error(`  wrote ${fmt.padEnd(4)} -> ${path}  (${size} bytes)`);
  }
}

try {
  main();
} catch (err) {
  console.error(err instanceof Error ? err.message : String(err));
  process.exit(1);
}
