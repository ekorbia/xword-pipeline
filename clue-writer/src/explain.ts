import Anthropic from "@anthropic-ai/sdk";
import { zodOutputFormat } from "@anthropic-ai/sdk/helpers/zod";
import { z } from "zod";
import type { CluedEntry, CluedPuzzle } from "./types.js";

import { MODELS } from "./models.js";

/**
 * Default model for the explain pass (see models.ts for the rationale).
 * Callers can override with the `model` option (e.g. `claude-opus-4-7` for
 * higher-quality wit on tricky puzzles).
 */
export const DEFAULT_EXPLAIN_MODEL: string = MODELS.explain;

/**
 * Whether the given model supports `thinking: { type: "adaptive" }` AND the
 * `output_config.effort` parameter. Both are gated to current Opus 4.6+/Sonnet
 * 4.6 per Anthropic's models docs (verified 2026-06). Haiku 4.5 rejects both.
 */
function supportsAdaptiveReasoning(model: string): boolean {
  return /^claude-opus-4-[6789]/.test(model) || /^claude-sonnet-4-6/.test(model);
}

/**
 * System prompt for the explainer pass. Explanations are shown AFTER the solver
 * finishes, so spoilers are fine — the goal is the little "aha" that makes each
 * answer click (the fact, the wordplay trick, the reference).
 */
export const EXPLAIN_GUIDE = `You are a friendly crossword explainer. A solver has just FINISHED the puzzle and wants to understand each answer.

For every entry, write ONE short sentence (about 8-25 words) explaining the connection between the clue and the answer:
- For a definitional/trivia clue, give the key fact or reference that links them.
- For wordplay (puns, anagrams, hidden words, "?"-style twists, abbreviations), name the trick so the solver sees how it worked.
- Be accurate, plain, and self-contained. Do NOT just restate the clue. Do NOT scold or pad.
- Since the puzzle is solved, you may freely mention the answer.

Return structured output: one object per entry { num, dir, answer, explanation }, echoing the answer so the mapping is unambiguous.`;

const ExplainItem = z.object({
  num: z.number().int(),
  dir: z.enum(["A", "D"]),
  answer: z.string(),
  explanation: z.string(),
});
const ExplainResponse = z.object({ explanations: z.array(ExplainItem) });

/** Render the per-puzzle explain prompt (answers + their clues). */
export function buildExplainMessage(puzzle: CluedPuzzle): string {
  const lines: string[] = [];
  lines.push(
    `Explain the ${puzzle.across.length + puzzle.down.length} answers in this ${puzzle.themed ? "themed" : "themeless"} crossword. Write one sentence per entry.`,
  );
  lines.push("");

  const fmt = (e: CluedEntry) => `  ${e.num}${e.dir} (${e.answer})${e.theme ? " [THEME]" : ""}: ${e.clue}`;
  lines.push("ACROSS (answer: clue):");
  for (const e of [...puzzle.across].sort((a, b) => a.num - b.num)) lines.push(fmt(e));
  lines.push("");
  lines.push("DOWN (answer: clue):");
  for (const e of [...puzzle.down].sort((a, b) => a.num - b.num)) lines.push(fmt(e));
  lines.push("");
  lines.push(
    "Return `explanations`: an array with one { num, dir, answer, explanation } object per entry above.",
  );
  return lines.join("\n");
}

export interface ExplainItemOut {
  num: number;
  dir: "A" | "D";
  answer: string;
  explanation: string;
}

export interface ExplainResult {
  items: ExplainItemOut[];
  warnings: string[];
  usage: { input: number; output: number; cacheRead: number; cacheWrite: number };
}

export interface ExplainOpts {
  /** Anthropic client (defaults to a fresh `new Anthropic()`). */
  client?: Anthropic;
  /** Model to use (defaults to {@link DEFAULT_EXPLAIN_MODEL}). */
  model?: string;
}

export async function explainPuzzle(
  puzzle: CluedPuzzle,
  opts: ExplainOpts = {},
): Promise<ExplainResult> {
  const client = opts.client ?? new Anthropic();
  const model = opts.model ?? DEFAULT_EXPLAIN_MODEL;
  const adaptiveOk = supportsAdaptiveReasoning(model);
  // Haiku 4.5 rejects both `thinking` (adaptive) and `output_config.effort`;
  // include them only on models that accept them. On Opus we drop effort to
  // `medium` for this pass — explanations don't need full deliberation.
  const response = await client.messages.parse({
    model,
    max_tokens: 16000,
    ...(adaptiveOk ? { thinking: { type: "adaptive" as const } } : {}),
    system: [{ type: "text", text: EXPLAIN_GUIDE, cache_control: { type: "ephemeral", ttl: "1h" } }],
    output_config: {
      ...(adaptiveOk ? { effort: "medium" as const } : {}),
      format: zodOutputFormat(ExplainResponse),
    },
    messages: [{ role: "user", content: buildExplainMessage(puzzle) }],
  });

  const parsed = response.parsed_output;
  if (!parsed) {
    throw new Error(`Model did not return structured explanations (stop_reason: ${response.stop_reason}).`);
  }

  const byKey = new Map<string, string>();
  for (const e of parsed.explanations) byKey.set(`${e.num}${e.dir}`, e.explanation.trim());

  const warnings: string[] = [];
  const items: ExplainItemOut[] = [];
  for (const e of [...puzzle.across, ...puzzle.down].sort((a, b) => a.num - b.num || a.dir.localeCompare(b.dir))) {
    const explanation = byKey.get(`${e.num}${e.dir}`);
    if (!explanation) {
      warnings.push(`missing explanation for ${e.num}${e.dir} (${e.answer})`);
      continue;
    }
    items.push({ num: e.num, dir: e.dir, answer: e.answer, explanation });
  }

  const u = response.usage;
  return {
    items,
    warnings,
    usage: {
      input: u.input_tokens,
      output: u.output_tokens,
      cacheRead: u.cache_read_input_tokens ?? 0,
      cacheWrite: u.cache_creation_input_tokens ?? 0,
    },
  };
}
