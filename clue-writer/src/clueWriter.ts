import Anthropic from "@anthropic-ai/sdk";
import { zodOutputFormat } from "@anthropic-ai/sdk/helpers/zod";
import { z } from "zod";
import type { CluedEntry, CluedPuzzle, Day, LibraryGrid, QAReport } from "./types.js";
import { DAY_GUIDANCE, STYLE_GUIDE } from "./styleGuide.js";
import { MODELS } from "./models.js";
import { streamStructured, STRUCTURED_MAX_TOKENS } from "./llm.js";

const MODEL = MODELS.clue;

/** answer (uppercased, letters only) appears in its own clue? */
function answerInClue(answer: string, clue: string): boolean {
  return clue.toUpperCase().replace(/[^A-Z]/g, "").includes(answer);
}

// Structured-output schema: one clue per answer, keyed by number + direction.
const ClueItem = z.object({
  num: z.number().int(),
  dir: z.enum(["A", "D"]),
  answer: z.string(),
  clue: z.string(),
});
const ClueResponse = z.object({ clues: z.array(ClueItem) });

/** Render the volatile per-puzzle prompt (answers + day guidance). */
export function buildUserMessage(grid: LibraryGrid, day: Day): string {
  const lines: string[] = [];
  lines.push(`Target day: ${day}`);
  lines.push(DAY_GUIDANCE[day]);
  lines.push("");

  const themeEntries = grid.entries.filter((e) => e.theme);
  if (grid.themed && themeEntries.length > 0) {
    lines.push(
      `This is a THEMED puzzle. The theme answers (clue these as the marquee entries, consistent with each other):`,
    );
    for (const e of themeEntries) {
      lines.push(`  - ${e.num}${e.dir} (${e.len}): ${e.answer}`);
    }
    lines.push("");
  }

  const fmt = (e: { num: number; dir: string; len: number; answer: string; theme: boolean }) =>
    `  ${e.num}${e.dir} (${e.len})${e.theme ? " [THEME]" : ""}: ${e.answer}`;

  const across = grid.entries.filter((e) => e.dir === "A").sort((a, b) => a.num - b.num);
  const down = grid.entries.filter((e) => e.dir === "D").sort((a, b) => a.num - b.num);

  lines.push(`Write exactly one clue for each of these ${grid.entries.length} answers, calibrated to ${day}.`);
  lines.push("");
  lines.push("ACROSS:");
  across.forEach((e) => lines.push(fmt(e)));
  lines.push("");
  lines.push("DOWN:");
  down.forEach((e) => lines.push(fmt(e)));
  lines.push("");
  lines.push(
    "Return the clues as structured output: an array with one object per answer { num, dir, answer, clue }. Echo each answer back so the mapping is unambiguous. Remember: never put the answer (or a word sharing its root) in its own clue — and no grid answer may appear in ANY clue in the puzzle; cross-check every clue against the full answer list above.",
  );
  return lines.join("\n");
}

export interface WriteCluesResult {
  across: CluedEntry[];
  down: CluedEntry[];
  warnings: string[];
  usage: { input: number; output: number; cacheRead: number; cacheWrite: number };
}

export async function writeClues(grid: LibraryGrid, day: Day, client = new Anthropic()): Promise<WriteCluesResult> {
  const userMessage = buildUserMessage(grid, day);

  const { output: parsed, usage } = await streamStructured(
    client,
    {
      model: MODEL,
      max_tokens: STRUCTURED_MAX_TOKENS,
      thinking: { type: "adaptive" },
      // Cache the (stable) style guide; the per-puzzle answers come after it.
      // 1h TTL so the cached style guide survives across multi-tier runs and
      // across puzzles when batch-generating in a session (write costs ~2x
      // input once; reads stay at ~0.1x).
      system: [{ type: "text", text: STYLE_GUIDE, cache_control: { type: "ephemeral", ttl: "1h" } }],
      output_config: {
        effort: "high",
        format: zodOutputFormat(ClueResponse),
      },
      messages: [{ role: "user", content: userMessage }],
    },
    ClueResponse,
    "clue writer",
  );

  // Map clues back to entries by num+dir.
  const clueByKey = new Map<string, string>();
  for (const c of parsed.clues) {
    clueByKey.set(`${c.num}${c.dir}`, c.clue.trim());
  }

  const warnings: string[] = [];
  const attach = (dir: "A" | "D"): CluedEntry[] =>
    grid.entries
      .filter((e) => e.dir === dir)
      .sort((a, b) => a.num - b.num)
      .map((e) => {
        const clue = clueByKey.get(`${e.num}${e.dir}`);
        if (!clue) {
          warnings.push(`missing clue for ${e.num}${e.dir} (${e.answer})`);
          return { ...e, clue: "[MISSING]" };
        }
        // Light fairness check: answer must not appear in its own clue.
        if (answerInClue(e.answer, clue)) {
          warnings.push(`clue for ${e.num}${e.dir} may contain the answer: "${clue}"`);
        }
        return { ...e, clue };
      });

  return {
    across: attach("A"),
    down: attach("D"),
    warnings,
    usage,
  };
}

// ---- Revision: feed QA findings back and rewrite only the flagged clues ----

const Revision = z.object({
  num: z.number().int(),
  dir: z.enum(["A", "D"]),
  answer: z.string(),
  clue: z.string(),
  addresses: z.string(), // which finding(s) this rewrite resolves
});
const Unresolved = z.object({
  location: z.string(),
  reason: z.string(), // why a re-clue can't fix it (e.g. needs a grid change)
});
const ReviseResponse = z.object({
  revisions: z.array(Revision),
  unresolved: z.array(Unresolved),
});

export function buildReviseMessage(puzzle: CluedPuzzle, report: QAReport): string {
  const lines: string[] = [];
  lines.push(`Target day: ${puzzle.day}`);
  lines.push(DAY_GUIDANCE[puzzle.day]);
  lines.push("");
  lines.push(
    "An editor reviewed this finished puzzle. REWRITE ONLY the clues needed to resolve the findings below. Leave every other clue exactly as it is. A rewritten clue must still obey all cluing rules and the day's difficulty, and must not reintroduce a problem elsewhere (no duplicate words across clues, and no grid answer — or word sharing its root — may appear in any clue).",
  );
  lines.push(
    "Some findings cannot be fixed by re-cluing — they require changing the GRID itself (e.g. a weak/nonword answer, or a duplicate answer). For those, do NOT invent a clue; instead list them under `unresolved` with a short reason.",
  );
  lines.push("");
  lines.push("Filled grid (# = black square):");
  lines.push(puzzle.fill.join("\n"));
  lines.push("");

  const block = (title: string, entries: CluedEntry[]) => {
    lines.push(title);
    for (const e of entries) {
      lines.push(`  ${e.num}${e.dir} = ${e.answer}${e.theme ? " [THEME]" : ""}  —  ${e.clue}`);
    }
  };
  block("CURRENT ACROSS (answer — clue):", puzzle.across);
  lines.push("");
  block("CURRENT DOWN (answer — clue):", puzzle.down);
  lines.push("");

  lines.push(`EDITOR FINDINGS (verdict: ${report.verdict}):`);
  for (const f of report.findings) {
    lines.push(`  - [${f.severity}/${f.category}] ${f.location}: ${f.issue}  (suggestion: ${f.suggestion})`);
  }
  lines.push("");
  lines.push(
    "Return `revisions` (only the clues you changed, each keyed by num+dir, echoing the answer, with `addresses` naming the finding it fixes) and `unresolved` (findings that need a grid change). Do not include unchanged clues.",
  );
  return lines.join("\n");
}

export interface ReviseResult {
  across: CluedEntry[];
  down: CluedEntry[];
  changed: { num: number; dir: "A" | "D"; answer: string; before: string; after: string; addresses: string }[];
  unresolved: { location: string; reason: string }[];
  warnings: string[];
  usage: { input: number; output: number; cacheRead: number };
}

export async function reviseClues(puzzle: CluedPuzzle, report: QAReport, client = new Anthropic()): Promise<ReviseResult> {
  const { output: parsed, usage } = await streamStructured(
    client,
    {
      model: MODEL,
      max_tokens: STRUCTURED_MAX_TOKENS,
      thinking: { type: "adaptive" },
      // Same cached system prompt as the clue writer → shares the prompt
      // cache. 1h TTL so the cached style guide survives across multi-tier
      // runs and across puzzles when batch-generating in a session.
      system: [{ type: "text", text: STYLE_GUIDE, cache_control: { type: "ephemeral", ttl: "1h" } }],
      output_config: { effort: "high", format: zodOutputFormat(ReviseResponse) },
      messages: [{ role: "user", content: buildReviseMessage(puzzle, report) }],
    },
    ReviseResponse,
    "clue reviser",
  );

  const revByKey = new Map<string, string>();
  const changed: ReviseResult["changed"] = [];
  for (const r of parsed.revisions) {
    revByKey.set(`${r.num}${r.dir}`, r.clue.trim());
  }

  const warnings: string[] = [];
  const merge = (entries: CluedEntry[], dir: "A" | "D"): CluedEntry[] =>
    entries.map((e) => {
      const next = revByKey.get(`${e.num}${e.dir}`);
      if (next === undefined || next === e.clue) return e;
      if (answerInClue(e.answer, next)) {
        warnings.push(`revised clue for ${e.num}${e.dir} still contains the answer: "${next}"`);
      }
      const addresses = parsed.revisions.find((r) => r.num === e.num && r.dir === e.dir)?.addresses ?? "";
      changed.push({ num: e.num, dir, answer: e.answer, before: e.clue, after: next, addresses });
      return { ...e, clue: next };
    });

  return {
    across: merge(puzzle.across, "A"),
    down: merge(puzzle.down, "D"),
    changed,
    unresolved: parsed.unresolved,
    warnings,
    usage,
  };
}
