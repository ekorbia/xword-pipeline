// CLI: brainstorm crossword theme sets with Claude that feed the xfill `theme`
// generator. Validates each proposed answer against the grid constraints and
// prints ready-to-run `theme` commands.
//
// Usage:
//   npm run theme-idea -- [--topic "..."] [--count 3] [--answers 3] [--out themes.json] [--dry-run]
//
// Requires ANTHROPIC_API_KEY (except with --dry-run).

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { buildIdeationMessage, ideateThemes, normalizeAnswer, themeCommand, type IdeationOpts } from "./themeIdeation.js";

const THEMES_OUT = "../out/themes/themes.json";

function parseArgs(argv: string[]): IdeationOpts & { out: string; dryRun: boolean } {
  const a = { topic: "", count: 3, answersPerTheme: 3, out: THEMES_OUT, dryRun: false };
  const rest = argv.slice(2);
  for (let i = 0; i < rest.length; i++) {
    const t = rest[i]!;
    if (t === "--topic") a.topic = rest[++i]!;
    else if (t === "--count") a.count = Number(rest[++i]);
    else if (t === "--answers") a.answersPerTheme = Number(rest[++i]);
    else if (t === "--out") a.out = rest[++i]!;
    else if (t === "--dry-run") a.dryRun = true;
    else throw new Error(`unexpected argument: ${t}`);
  }
  return a;
}

async function main() {
  const args = parseArgs(process.argv);
  const opts: IdeationOpts = { topic: args.topic, count: args.count, answersPerTheme: args.answersPerTheme };

  if (args.dryRun) {
    console.error("--- DRY RUN: ideation prompt (no API call) ---\n");
    console.log(buildIdeationMessage(opts));
    return;
  }
  if (!process.env.ANTHROPIC_API_KEY) {
    console.error("error: ANTHROPIC_API_KEY is not set. Export it, or use --dry-run.");
    process.exit(1);
  }

  const t0 = Date.now();
  const { ideas, usage } = await ideateThemes(opts);
  const secs = ((Date.now() - t0) / 1000).toFixed(1);
  console.error(`proposed ${ideas.themes.length} theme set(s) in ${secs}s (cache read ${usage.cacheRead} tok)\n`);

  ideas.themes.forEach((theme, i) => {
    console.error(`=== Theme ${i + 1}: ${theme.concept} ===`);
    if (theme.revealer) console.error(`revealer: ${theme.revealer}`);
    console.error(`why: ${theme.rationale}`);
    const valid: string[] = [];
    for (const ans of theme.answers) {
      const n = normalizeAnswer(ans.text);
      const mark = n.ok ? "ok " : "BAD";
      const lenNote = n.len !== ans.len ? ` (model said ${ans.len}, actual ${n.len})` : "";
      console.error(`  [${mark}] ${n.normalized} (${n.len})${lenNote}${n.ok ? "" : ` — ${n.reason}`}  · ${ans.note}`);
      if (n.ok) valid.push(n.normalized);
    }
    if (valid.length === 0) {
      console.error(`  (no buildable answers in this set)`);
    } else if (valid.length > 4) {
      console.error(`  note: ${valid.length} valid answers; the generator places up to 4 — using the first 4 below.`);
      console.error(`  run: ${themeCommand(valid.slice(0, 4))}`);
    } else {
      console.error(`  run: ${themeCommand(valid)}`);
    }
    console.error("");
  });

  if (args.out) {
    mkdirSync(dirname(args.out), { recursive: true });
    writeFileSync(args.out, JSON.stringify(ideas, null, 2));
    console.error(`wrote ideas -> ${args.out}`);
  }
}

main().catch((err) => {
  console.error(err instanceof Error ? err.message : String(err));
  process.exit(1);
});
