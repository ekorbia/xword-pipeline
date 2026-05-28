import Anthropic from "@anthropic-ai/sdk";
import { zodOutputFormat } from "@anthropic-ai/sdk/helpers/zod";
import { z } from "zod";
import type { CluedPuzzle, QAReport } from "./types.js";

const MODEL = "claude-opus-4-7";

export const EDITOR_GUIDE = `You are the test-solving editor for a New York Times-caliber crossword. You receive a FINISHED puzzle — the filled grid plus every clue — and produce a rigorous editorial review. Your job is to catch what a careful editor catches before publication. Be specific, fair, and concrete; every finding must name a location and a fix.

# What to check

1. FILL QUALITY. Flag genuinely weak entries: obscure crosswordese, awkward partials, made-up-looking strings, random Roman numerals/abbreviations, or anything that isn't a real, recognizable word/name/phrase. Short glue (3-4 letters) gets some latitude; long entries get little.
2. DUPLICATES. The same word may not appear twice as an answer, and an answer word (or a clear root of it) must not appear in ANY clue anywhere in the puzzle. Flag shared roots across the whole grid (e.g. an answer SAND and a clue containing "sandy").
3. CLUE ACCURACY. Each clue must correctly and fairly indicate its answer: right definition, right facts, right part of speech, agreement in tense/number, correct abbreviation/foreign signals. Flag factual errors, POS/tense mismatches, missing "Abbr."/"for short" on shortened answers, and clues that are simply wrong.
4. FAIRNESS / CROSSINGS. Flag unfair crossings: two obscure entries crossing at a hard-to-guess letter (a "Natick"), especially proper-noun × proper-noun.
5. DIFFICULTY CALIBRATION. Clues must match the stated day. Flag clues that are too hard for an early-week puzzle (gratuitous trivia/misdirection) or too easy/hand-holding for a late-week one. Note unmarked puns (missing "?") and overused "?" clues.
6. BREAKFAST TEST. Flag answers or clues that are grim, gross, slurs, or otherwise unfit for a general morning audience.
7. THEME. For themed puzzles, verify the theme answers are consistent and their clues honor the theme; flag a theme answer that breaks the pattern or a revealer reference that points to the wrong number.
8. STYLE. Flag repeated clue gimmicks/wording, terminal periods on non-abbreviation clues, answer wrapped in stray quotes, and other house-style slips.

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

export function buildReviewMessage(p: CluedPuzzle): string {
  const lines: string[] = [];
  lines.push(`Puzzle type: ${p.themed ? "THEMED" : "themeless"}   Target day: ${p.day}`);
  if (p.themed && p.themes.length) {
    lines.push(`Stated theme answers: ${p.themes.join(", ")}`);
  }
  lines.push("");
  lines.push("Filled grid (# = black square):");
  lines.push(p.fill.join("\n"));
  lines.push("");
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
  lines.push("Review this puzzle per your editorial checklist and return the structured report.");
  return lines.join("\n");
}

export async function reviewPuzzle(p: CluedPuzzle, client = new Anthropic()): Promise<{ report: QAReport; usage: { input: number; output: number; cacheRead: number } }> {
  const response = await client.messages.parse({
    model: MODEL,
    max_tokens: 16000,
    thinking: { type: "adaptive" },
    system: [{ type: "text", text: EDITOR_GUIDE, cache_control: { type: "ephemeral", ttl: "1h" } }],
    output_config: { effort: "high", format: zodOutputFormat(QAReportSchema) },
    messages: [{ role: "user", content: buildReviewMessage(p) }],
  });
  if (!response.parsed_output) {
    throw new Error(`Reviewer returned no structured report (stop_reason: ${response.stop_reason}).`);
  }
  const u = response.usage;
  return {
    report: response.parsed_output,
    usage: { input: u.input_tokens, output: u.output_tokens, cacheRead: u.cache_read_input_tokens ?? 0 },
  };
}
