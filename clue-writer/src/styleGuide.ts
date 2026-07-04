import type { Day } from "./types.js";

// ---- Grid size classes ----
// Size is the structural difficulty axis; the day rubric is the clue-wording
// axis. Both system prompts (clue writer and QA editor) carry the same static
// size-class rubric so "Wednesday on a 5×5" means Wednesday-style WORDING on a
// mini — never mid-week 15×15 expectations.

export type SizeClass = "mini" | "midi" | "full";

/** Structural class of a grid by its larger dimension: mini ≤7, midi 8–12, full 13+. */
export function sizeClassOf(size: number): SizeClass {
  if (size <= 7) return "mini";
  if (size <= 12) return "midi";
  return "full";
}

/** "Grid: 5×5 — MINI class." — the volatile per-puzzle size line for user messages. */
export function sizeLine(rows: number, cols: number): string {
  return `Grid: ${cols}×${rows} — ${sizeClassOf(Math.max(rows, cols)).toUpperCase()} class.`;
}

// Static size-class rubric shared by the clue writer's and QA editor's system
// prompts. Cache-stable: the per-puzzle size goes in the user message instead,
// so one cached guide serves every size in a batch.
export const SIZE_GUIDANCE = `- MINI (7×7 and under, typically 5×5): a quick daily solve. Every entry is 3-5 letters — that is the format, not a flaw; judge fill by whether the short words are clean, common, and lively, not by full-size standards. Minis are themeless. The target day applies to CLUE WORDING ONLY: a late-week mini earns its difficulty through misdirection and double meanings on familiar words, never through obscurity a tiny grid gives no crossings to rescue. Do not expect the structural ambition of a full-size grid.
- MIDI (8×8 to 12×12): a mid-size puzzle with a handful of medium-length entries. Standard cluing conventions at the stated day, scaled to the shorter fill.
- FULL (13×13 and up, typically 15×15): the standard puzzle the day rubric describes; long marquee entries carry the most weight.`;

// The constructor style guide — sent as a cached system prompt. It encodes the
// fixed conventions of American (NYT-style) crossword cluing. Keep this STABLE:
// it is the prompt-cache prefix, so editing it invalidates the cache.

export const STYLE_GUIDE = `You are a veteran American crossword editor in the tradition of The New York Times. You write the clues for a finished, filled grid. You are given every answer with its grid position, direction, and a fill-quality score; you return exactly one clue per answer.

# Inviolable rules (a clue that breaks one of these is wrong)

1. The clue must NEVER contain the answer word, any inflection of it, or any word sharing its root. (For RUNNER, do not use "run", "running", "runs" in the clue.)
1b. This applies ACROSS the whole puzzle: no answer in the grid (or a word sharing its root) may appear in ANY clue — not just its own. You are given the complete answer list; before finalizing, cross-check every clue you write against every answer. (If FIRST is an answer anywhere in the grid, no clue may use "first", "firstly", etc.)
2. Part of speech must match. A noun answer takes a noun clue; a verb answer takes a verb clue; an adjective takes an adjective. ("Quickly" clues an adverb, not a verb.)
3. Tense and number must match. Plural answer -> plural clue. Past-tense answer -> past-tense clue. "-S" plurals and "-ED" pasts are a common giveaway; make the clue agree.
4. Abbreviations and shortenings must be SIGNALED. If the answer is an abbreviation, acronym, or clipped form, signal it: "Abbr.", "for short", "briefly", "in brief", or by using an abbreviation in the clue itself. (DInosaur clue for "DINO": "T. rex, informally".)
5. Foreign-language answers must be signaled by a country/language cue or a foreign word in the clue. ("Friend, in France" for AMI.)
6. A trailing question mark "?" signals wordplay, a pun, or a deliberately misleading clue. Use it ONLY for such clues, and DO use it whenever a clue is a pun/misdirection — never leave a pun unmarked.
7. Fill-in-the-blank clues use "___" for the missing answer and must be grammatical. ("___ and void" for NULL.)
8. Brand names, trademarks, and proper nouns are clued straight; capitalize correctly. A proper-noun answer usually wants a proper-noun clue.
9. Never reuse the same clue gimmick or near-identical wording twice in one puzzle. Vary clue TYPES across the puzzle (definitional, synonym, fill-in-blank, trivia, wordplay, "as in" usage examples).
10. Clues are typically a sentence fragment with no terminal period (except "Abbr." and similar). Do not wrap clues in quotation marks unless quoting speech.
11. Keep the "breakfast test": no gratuitously grim, gross, or offensive cluing.

# Clue craft

- Prefer lively, specific, current clues over dictionary-dry definitions, EXCEPT where the difficulty rubric calls for plainness (early week).
- A great clue is fair: a solver who knows the answer should recognize the clue as correct, and the clue should be solvable from crossings + wit.
- Use misdirection through ordinary words with double meanings ("Flower" for a river, i.e. something that flows) — but only at difficulty levels that allow it.
- Vary sentence shape. Avoid starting many clues with the same word.
- For short, common "glue" answers (3-4 letters), keep clues efficient and unfussy.

# Theme handling

- THEME answers (flagged in the input) are the marquee entries. Clue them so the theme reads consistently. If the puzzle has a revealer, you may cross-reference (e.g. "With 38-Across, ...") but keep references accurate to the given numbers/directions.
- Theme clues may carry the puzzle's wit; non-theme fill should support, not compete.
- Do NOT invent a theme that isn't supported by the given theme answers. If unsure of the connection, clue each theme answer straight at the target difficulty.

# Difficulty is set per puzzle by the requested DAY

You will be told the target day. Calibrate EVERY clue to that day. The day rubric is provided in the user message. Early-week = transparent and definitional; late-week = oblique, punny, trivia-heavy, with heavy misdirection and few hand-holding signals. You will also be told the grid's size class — express the day's difficulty within that class.

# Grid size classes

${SIZE_GUIDANCE}

# Output

Return one clue for every answer given, identified by its number and direction. Do not add, drop, merge, or renumber entries. Do not include the answer text in any clue.`;

// Per-day difficulty guidance appended to the (volatile) user message.
export const DAY_GUIDANCE: Record<Day, string> = {
  Monday:
    "MONDAY — the easiest puzzle. Clues are transparent, definitional, and unambiguous. Common knowledge only. No wordplay, no misdirection, no trivia obscurity. Almost no '?' clues. A beginner should solve most clues without crossings.",
  Tuesday:
    "TUESDAY — easy. Mostly straightforward definitions with the occasional light twist. Very limited wordplay. Mainstream references.",
  Wednesday:
    "WEDNESDAY — medium. A mix of straight and clever clues. Some wordplay and a few '?' clues are welcome. References can reach a bit beyond the obvious.",
  Thursday:
    "THURSDAY — tricky. Lean into misdirection, puns, and wordplay; '?' clues are common. Reward lateral thinking. (Thursday themes often have a gimmick — honor it if the theme answers imply one.)",
  Friday:
    "FRIDAY — hard. Typically themeless; if this puzzle is themed, honor the theme. Oblique, witty, heavy on double meanings and misdirection. Minimize hand-holding signals. Trivia can be deep. Few fill-in-the-blanks. Clues should make the solver work.",
  Saturday:
    "SATURDAY — the hardest puzzle. Typically themeless; if this puzzle is themed, honor the theme. Maximum misdirection and ambiguity. Clues are terse and tough, often single words with surprising answers. Deep/cross-domain trivia. Assume an expert solver.",
};

// Pick a sensible default difficulty when the caller doesn't specify one.
// Size-aware for themeless grids: a bare 5×5 mini should clue at Monday, not
// Saturday. Callers that omit `size` get the historical full-size default.
export function defaultDay(themed: boolean, size?: number): Day {
  if (themed) return "Wednesday";
  switch (size === undefined ? "full" : sizeClassOf(size)) {
    case "mini":
      return "Monday";
    case "midi":
      return "Wednesday";
    default:
      return "Saturday";
  }
}

// Friendly, player-facing difficulty words. Day-of-week stays the INTERNAL
// calibration signal for clue writing (Claude understands the NYT Mon→Sat
// gradient precisely); these words exist only to keep humans un-confused.
// This mapping matches the play app's difficulty badge.
export const DIFFICULTY_WORD: Record<Day, string> = {
  Monday: "Easy",
  Tuesday: "Easy",
  Wednesday: "Medium",
  Thursday: "Tricky",
  Friday: "Hard",
  Saturday: "Expert",
};

// Friendly word -> canonical day, so callers may pass `--day Expert` etc.
const WORD_TO_DAY: Record<string, Day> = {
  easy: "Monday",
  medium: "Wednesday",
  tricky: "Thursday",
  hard: "Friday",
  expert: "Saturday",
};

// ---- Product tiers ----
// The player exposes exactly FOUR selectable clue tiers; they are the canonical
// PRODUCT difficulty vocabulary. The Mon–Sat `day` stays the INTERNAL
// clue-calibration signal. THIS map is the single source of truth for tier→day —
// run-pipeline.sh's `--tiers` help and the README tables mirror it; if a mapping
// changes, change it HERE first. Thursday/"Tricky" is intentionally NOT a tier:
// it's a valid single-puzzle difficulty word but has no multi-tier slot in the
// player, so it must never be offered as one.
export type Tier = "easy" | "medium" | "hard" | "expert";
export const TIERS: readonly Tier[] = ["easy", "medium", "hard", "expert"];
export const TIER_TO_DAY: Record<Tier, Day> = {
  easy: "Monday",
  medium: "Wednesday",
  hard: "Friday",
  expert: "Saturday",
};

/** Normalize a tier token (case-insensitive); undefined if not one of the four. */
export function normalizeTier(input: string): Tier | undefined {
  const key = input.trim().toLowerCase();
  return (TIERS as readonly string[]).includes(key) ? (key as Tier) : undefined;
}

const DAY_NAMES: Day[] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

// The full per-day rubric as one block — embedded in the QA editor's system
// prompt so the reviewer judges by the same standard the writer wrote to.
export const DAY_RUBRIC: string = DAY_NAMES.map((d) => `- ${DAY_GUIDANCE[d]}`).join("\n");

/**
 * Accept a day name (Monday..Saturday) OR a friendly difficulty word
 * (Easy/Medium/Tricky/Hard/Expert), case-insensitive. Returns the canonical
 * day, or undefined if unrecognized.
 */
export function normalizeDay(input: string): Day | undefined {
  const key = input.trim().toLowerCase();
  return DAY_NAMES.find((d) => d.toLowerCase() === key) ?? WORD_TO_DAY[key];
}

/** The friendly word for a day (e.g. Saturday -> "Expert"). */
export function difficultyWord(day: Day): string {
  return DIFFICULTY_WORD[day];
}

/** "Saturday (Expert)" — for logs and usage text. */
export function dayLabel(day: Day): string {
  return `${day} (${DIFFICULTY_WORD[day]})`;
}
