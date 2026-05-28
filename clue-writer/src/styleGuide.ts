import type { Day } from "./types.js";

// The constructor style guide — sent as a cached system prompt. It encodes the
// fixed conventions of American (NYT-style) crossword cluing. Keep this STABLE:
// it is the prompt-cache prefix, so editing it invalidates the cache.

export const STYLE_GUIDE = `You are a veteran American crossword editor in the tradition of The New York Times. You write the clues for a finished, filled grid. You are given every answer with its grid position, direction, and a fill-quality score; you return exactly one clue per answer.

# Inviolable rules (a clue that breaks one of these is wrong)

1. The clue must NEVER contain the answer word, any inflection of it, or any word sharing its root. (For RUNNER, do not use "run", "running", "runs" in the clue.)
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

You will be told the target day. Calibrate EVERY clue to that day. The day rubric is provided in the user message. Early-week = transparent and definitional; late-week = oblique, punny, trivia-heavy, with heavy misdirection and few hand-holding signals.

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
    "FRIDAY — hard, themeless. Oblique, witty, heavy on double meanings and misdirection. Minimize hand-holding signals. Trivia can be deep. Few fill-in-the-blanks. Clues should make the solver work.",
  Saturday:
    "SATURDAY — the hardest puzzle, themeless. Maximum misdirection and ambiguity. Clues are terse and tough, often single words with surprising answers. Deep/cross-domain trivia. Assume an expert solver.",
};

// Pick a sensible default difficulty when the caller doesn't specify one.
export function defaultDay(themed: boolean): Day {
  return themed ? "Wednesday" : "Saturday";
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

const DAY_NAMES: Day[] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

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
