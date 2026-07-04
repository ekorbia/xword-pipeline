import Anthropic from "@anthropic-ai/sdk";
import { zodOutputFormat } from "@anthropic-ai/sdk/helpers/zod";
import { z } from "zod";
import type { CluedPuzzle, QAReport } from "./types.js";
import { DAY_RUBRIC, SIZE_GUIDANCE, sizeLine } from "./styleGuide.js";
import { MODELS } from "./models.js";
import { streamStructured, STRUCTURED_MAX_TOKENS } from "./llm.js";

const MODEL = MODELS.qa;

// A review can be scoped so multi-tier puzzles don't pay for the same grid
// analysis N times. GRID findings (fill, duplicate answers, Naticks, theme
// answers, answer-level breakfast) are identical across every clue set on a
// grid, so they're reviewed ONCE. CLUE findings (accuracy, difficulty, style,
// answer-in-clue, theme cluing) are per tier. "full" reviews everything (the
// single-tier default). The system prompt is shared across all three scopes,
// so the prompt cache is reused; only this volatile directive changes.
export type QAScope = "grid" | "clue" | "full";

const SCOPE_DIRECTIVE: Record<QAScope, string> = {
  grid:
    "SCOPE — GRID-LEVEL ONLY. Review what is intrinsic to the filled grid and its ANSWERS, independent of how any answer is clued: FILL quality, DUPLICATE answers / shared roots between two answers, unfair CROSSINGS (Naticks), ANSWERS that fail the breakfast test, and theme-ANSWER consistency. These are identical for every clue set written on this grid, so they are reviewed once, here. Do NOT judge clue wording, accuracy, difficulty, style, or answer-in-clue duplicates — a separate per-tier pass owns those. You are given the answers only (no clues). Use only these categories: fill, duplicate, fairness, breakfast-test, theme.",
  clue:
    "SCOPE — CLUE-LEVEL. The grid and its answers were reviewed separately; treat the FILL as settled. Review only the CLUES against the target day and size class: clue ACCURACY, DIFFICULTY calibration, an answer (or its root) appearing in any clue, clue wording that fails the breakfast test, whether theme CLUES honor the theme, and STYLE. Do NOT report weak fill, duplicate answers, or Natick geometry unless a specific CLUE is the cause. Use only these categories: clue-accuracy, difficulty, duplicate, breakfast-test, theme, style.",
  full:
    "SCOPE — FULL. Review every category in your checklist (fill, duplicate, clue-accuracy, fairness, difficulty, breakfast-test, theme, style).",
};

export const EDITOR_GUIDE = `You are the test-solving editor for a New York Times-caliber crossword. You receive a FINISHED puzzle — the filled grid plus every clue — and produce a rigorous editorial review. Your job is to catch what a careful editor catches before publication. Be specific, fair, and concrete; every finding must name a location and a fix.

# What to check

1. FILL QUALITY. Flag genuinely weak entries: obscure crosswordese, awkward partials, made-up-looking strings, random Roman numerals/abbreviations, or anything that isn't a real, recognizable word/name/phrase. Short glue (3-4 letters) gets some latitude; long entries get little. Judge against the puzzle's size class (see below): in a MINI every entry is short — grade the short fill on cleanliness and liveliness, not on being short.
2. DUPLICATES. The same word may not appear twice as an answer, and an answer word (or a clear root of it) must not appear in ANY clue anywhere in the puzzle. Flag shared roots across the whole grid (e.g. an answer SAND and a clue containing "sandy").
3. CLUE ACCURACY. Each clue must correctly and fairly indicate its answer: right definition, right facts, right part of speech, agreement in tense/number, correct abbreviation/foreign signals. Flag factual errors, POS/tense mismatches, missing "Abbr."/"for short" on shortened answers, and clues that are simply wrong.
4. FAIRNESS / CROSSINGS. Flag unfair crossings: two obscure entries crossing at a hard-to-guess letter (a "Natick"), especially proper-noun × proper-noun.
5. DIFFICULTY CALIBRATION. Clues must match the stated day per the day rubric below, interpreted within the puzzle's size class. Flag clues that are too hard for an early-week puzzle (gratuitous trivia/misdirection) or too easy/hand-holding for a late-week one. Note unmarked puns (missing "?") and overused "?" clues. Do not flag a small puzzle for lacking full-size structural difficulty — on a mini, the day lives in clue wording alone.
6. BREAKFAST TEST. Flag answers or clues that are grim, gross, slurs, or otherwise unfit for a general morning audience.
7. THEME. For themed puzzles, verify the theme answers are consistent and their clues honor the theme; flag a theme answer that breaks the pattern or a revealer reference that points to the wrong number.
8. STYLE. Flag repeated clue gimmicks/wording, terminal periods on non-abbreviation clues, answer wrapped in stray quotes, and other house-style slips.

# Day rubric (the standard the clues were written to)

${DAY_RUBRIC}

# Grid size classes

${SIZE_GUIDANCE}

# Severity
- high: must fix before publishing (wrong clue, duplicate, unfair Natick, offensive content, non-word fill in a long slot).
- medium: should fix (weak short fill, mild difficulty miscalibration, a stretchy clue).
- low: nit (style polish, a marginally better clue available).

# Verdict
- "ready": publishable as-is or with only low-severity nits.
- "minor-revisions": a handful of medium issues, no highs.
- "needs-work": any high-severity issue, or many mediums.

Report findings ordered by severity (high first). Do not invent problems to pad the list; if the puzzle is clean, say so and return few or no findings. Always include a one-paragraph summary.`;

const QAFindingSchema = z.object({
  location: z.string(),
  severity: z.enum(["high", "medium", "low"]),
  category: z.enum(["fill", "clue-accuracy", "duplicate", "difficulty", "fairness", "breakfast-test", "theme", "style"]),
  issue: z.string(),
  suggestion: z.string(),
});
const QAReportSchema = z.object({
  verdict: z.enum(["ready", "minor-revisions", "needs-work"]),
  summary: z.string(),
  findings: z.array(QAFindingSchema),
});

export function buildReviewMessage(p: CluedPuzzle, scope: QAScope = "full"): string {
  const lines: string[] = [];
  lines.push(`Puzzle type: ${p.themed ? "THEMED" : "themeless"}   Target day: ${p.day}`);
  lines.push(sizeLine(p.fill.length, p.fill[0]?.length ?? p.fill.length));
  if (p.themed && p.themes.length) {
    lines.push(`Stated theme answers: ${p.themes.join(", ")}`);
  }
  lines.push("");
  lines.push(SCOPE_DIRECTIVE[scope]);
  lines.push("");
  lines.push("Filled grid (# = black square):");
  lines.push(p.fill.join("\n"));
  lines.push("");

  // The grid review is clue-independent (so its findings apply to every tier),
  // so it gets the answers only — smaller prompt, and no clue text to grade.
  if (scope === "grid") {
    const block = (title: string, entries: CluedPuzzle["across"]) => {
      lines.push(title);
      for (const e of entries) {
        lines.push(`  ${e.num}${e.dir} = ${e.answer}${e.theme ? " [THEME]" : ""}`);
      }
    };
    block("ACROSS answers:", p.across);
    lines.push("");
    block("DOWN answers:", p.down);
    lines.push("");
    lines.push("Review the GRID and answers per the scope above and return the structured report.");
    return lines.join("\n");
  }

  const block = (title: string, entries: CluedPuzzle["across"]) => {
    lines.push(title);
    for (const e of entries) {
      lines.push(`  ${e.num}${e.dir} = ${e.answer}${e.theme ? " [THEME]" : ""}  —  ${e.clue}`);
    }
  };
  block("ACROSS (answer — clue):", p.across);
  lines.push("");
  block("DOWN (answer — clue):", p.down);
  lines.push("");
  lines.push("Review this puzzle per the scope above and return the structured report.");
  return lines.join("\n");
}

export async function reviewPuzzle(
  p: CluedPuzzle,
  scope: QAScope = "full",
  client = new Anthropic(),
): Promise<{ report: QAReport; usage: { input: number; output: number; cacheRead: number } }> {
  const { output: report, usage } = await streamStructured(
    client,
    {
      model: MODEL,
      max_tokens: STRUCTURED_MAX_TOKENS,
      thinking: { type: "adaptive" },
      system: [{ type: "text", text: EDITOR_GUIDE, cache_control: { type: "ephemeral", ttl: "1h" } }],
      output_config: { effort: "high", format: zodOutputFormat(QAReportSchema) },
      messages: [{ role: "user", content: buildReviewMessage(p, scope) }],
    },
    QAReportSchema,
    `QA reviewer (${scope})`,
  );
  return { report, usage };
}
