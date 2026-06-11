import Anthropic from "@anthropic-ai/sdk";
import { zodOutputFormat } from "@anthropic-ai/sdk/helpers/zod";
import { z } from "zod";
import type { ThemeIdeas } from "./types.js";
import { MODELS } from "./models.js";
import { streamStructured, STRUCTURED_MAX_TOKENS } from "./llm.js";

const MODEL = MODELS.themeIdea;

// The constructor brief. Encodes the GRID CONSTRAINTS imposed by the xfill
// `theme` generator so the proposed answers are actually buildable.
export const IDEATION_GUIDE = `You are a New York Times crossword theme constructor. You invent THEME SETS for 15x15 themed puzzles. Each theme set is a small group of long across answers that share one tight, elegant gimmick, ideally tied together by a revealer.

# What makes a good theme
- ONE precise, consistent gimmick across every theme answer — hidden words, puns, add/change/drop a letter, a shared category, reinterpreted phrases, etc. The connection must be exact, not loose.
- Theme answers are lively, real, in-the-language phrases or names — the kind a solver enjoys uncovering.
- Prefer a revealer: a final theme answer (or a short entry) that names or explains the trick.
- Aim for consistency in how the gimmick applies (e.g. the hidden word is always at the same position, or every base phrase changes the same way).

# HARD grid constraints (a set that violates these cannot be built)
- 1 to 4 theme answers per puzzle. (The generator places up to 4.)
- Each theme answer is a SINGLE across entry: letters A-Z only, NO spaces or punctuation, written as one uppercase run (e.g. WAITINGFORGODOT, GETTOSECONDBASE).
- Each answer length must be between 3 and 15, and must NOT be exactly 12 — a 12-letter answer cannot be placed as a single across in a 15-wide grid.
- FILL FEASIBILITY: lengths 7-11 are the sweet spot — long enough to be marquee entries, short enough that the grid around them fills reliably. Lengths 13-15 are allowed but make the surrounding fill MUCH harder (a near-full-width answer and its symmetric mirror slot cross almost every down word) — use at most one 13-15 answer per set, and only when the gimmick demands it. Avoid short (≤6) theme answers unless the gimmick truly needs them.
- Count every answer's letters carefully and report the exact length. Double-check none is 12.

# Output
Propose the requested number of DISTINCT candidate theme sets. For each: the concept (the gimmick in one sentence), a revealer if there is one (else empty string), the list of answers with each answer's exact letter count and a short note on how it fits, and a one-line rationale for why the theme is good. Make the answers genuinely buildable real phrases — not invented strings.`;

const ThemeAnswerSchema = z.object({
  text: z.string(),
  len: z.number().int(),
  note: z.string(),
});
const ThemeIdeaSchema = z.object({
  concept: z.string(),
  revealer: z.string(),
  answers: z.array(ThemeAnswerSchema),
  rationale: z.string(),
});
const ThemeIdeasSchema = z.object({ themes: z.array(ThemeIdeaSchema) });

export interface IdeationOpts {
  topic?: string;
  count: number; // number of candidate theme sets
  answersPerTheme: number; // target theme answers per set
}

export function buildIdeationMessage(opts: IdeationOpts): string {
  const lines: string[] = [];
  lines.push(`Propose ${opts.count} distinct candidate theme set(s) for a 15x15 themed crossword.`);
  lines.push(`Target ${opts.answersPerTheme} theme answers per set (1-4 allowed; a revealer may be one of them or extra).`);
  if (opts.topic && opts.topic.trim()) {
    lines.push(`Theme direction / seed: ${opts.topic.trim()}`);
  } else {
    lines.push(`No specific topic — propose your most elegant, original ideas.`);
  }
  lines.push("");
  lines.push("Remember the hard grid constraints: A-Z only, no spaces, each length 3-15 and never exactly 12. Favor 7-11 letters (fills reliably); at most one 13-15 answer per set. Count letters exactly.");
  return lines.join("\n");
}

export async function ideateThemes(opts: IdeationOpts, client = new Anthropic()): Promise<{ ideas: ThemeIdeas; usage: { input: number; output: number; cacheRead: number } }> {
  const { output: ideas, usage } = await streamStructured(
    client,
    {
      model: MODEL,
      max_tokens: STRUCTURED_MAX_TOKENS,
      thinking: { type: "adaptive" },
      system: [{ type: "text", text: IDEATION_GUIDE, cache_control: { type: "ephemeral", ttl: "1h" } }],
      // Medium effort: brainstorming with a human filter at the end, and
      // answer lengths are re-validated client-side — full deliberation isn't
      // needed and mostly burns thinking tokens.
      output_config: { effort: "medium", format: zodOutputFormat(ThemeIdeasSchema) },
      messages: [{ role: "user", content: buildIdeationMessage(opts) }],
    },
    ThemeIdeasSchema,
    "theme ideation",
  );
  return { ideas, usage };
}

// ---- client-side validation against the generator's constraints ----

export interface NormalizedAnswer {
  raw: string;
  normalized: string;
  len: number;
  ok: boolean;
  reason?: string;
}

export function normalizeAnswer(text: string): NormalizedAnswer {
  const normalized = text.toUpperCase().replace(/[^A-Z]/g, "");
  const len = normalized.length;
  let ok = true;
  let reason: string | undefined;
  if (len < 3) {
    ok = false;
    reason = "too short (<3)";
  } else if (len > 15) {
    ok = false;
    reason = "too long (>15)";
  } else if (len === 12) {
    ok = false;
    reason = "length 12 is unplaceable as a single across in a 15-wide grid";
  }
  return { raw: text, normalized, len, ok, reason };
}

/** Build a ready-to-run `theme` command for a set of normalized answers. */
export function themeCommand(answers: string[]): string {
  const flags = answers.map((a) => `--theme ${a}`).join(" ");
  return `(cd ../fill-engine && ./target/release/theme ${flags} --blocks 44 --candidates 150 --time 5)`;
}
