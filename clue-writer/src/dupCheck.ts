// Deterministic answer-in-clue duplicate detection: the QA editor's rule is
// that no grid answer (or a word sharing its root) may appear in ANY clue in
// the puzzle. The clue writer is prompted to comply, but one-shot compliance
// over ~78 clues is imperfect — this validator catches the misses BEFORE the
// QA step, so violations get auto-revised instead of becoming high-severity
// QA findings (historically the most common kind).
//
// The stemmer is a TS port of fill-engine/crates/xfill-core/src/dup.rs —
// keep the two in sync. Same deliberate limits: inflections only (plurals,
// tenses, comparatives, ordinals, adverbs), no irregular forms or compounds.

import type { CluedEntry, QAFinding } from "./types.js";

/** Common function words exempt from the dup rule (standard editor practice —
 * a grid answer like THE shouldn't poison every clue containing "the"). */
const EXEMPT = new Set([
  "THE", "AND", "FOR", "ARE", "BUT", "NOT", "WITH", "ITS", "HIS", "HER",
  "WAS", "HAS", "HAD", "WHO", "THAT", "THIS", "FROM", "INTO", "YOUR",
  "THEY", "THEM", "WHAT", "WHEN", "WHERE", "WHICH", "MAY", "CAN",
]);

/** Stem an uppercase A-Z word by iteratively stripping one inflectional
 * suffix per pass, never shrinking below 3 letters. Mirrors dup.rs. */
export function stem(word: string): string {
  let w = word;
  for (;;) {
    const next = stripOne(w);
    if (next.length === w.length) return w;
    w = next;
  }
}

function stripOne(w: string): string {
  const n = w.length;
  const keep = (k: number) => n >= k + 3;

  if (keep(2) && w.endsWith("IES")) return `${w.slice(0, n - 3)}Y`;
  for (const suf of ["ING", "EST", "ED", "ER"]) {
    if (keep(suf.length) && w.endsWith(suf)) {
      let s = w.slice(0, n - suf.length);
      const last = s[s.length - 1]!;
      if (s.length >= 4 && last === s[s.length - 2] && !"AEIOU".includes(last)) {
        s = s.slice(0, -1);
      }
      return s;
    }
  }
  for (const suf of ["ES", "TH", "LY"]) {
    if (keep(suf.length) && w.endsWith(suf)) return w.slice(0, n - suf.length);
  }
  if (keep(1) && w.endsWith("S") && !w.endsWith("SS")) return w.slice(0, n - 1);
  // Prefixes, only when a substantial root (>= 4 letters) remains:
  // UNEVENLY -> EVENLY -> EVEN; but UNION stays (ION is 3 letters).
  for (const pre of ["OVER", "OUT", "PRE", "NON", "DIS", "MIS", "UN", "RE"]) {
    if (n >= pre.length + 4 && w.startsWith(pre)) return w.slice(pre.length);
  }
  return w;
}

/** Word tokens of a clue, uppercased, length >= 3 (blanks `___`, numbers and
 * punctuation drop out naturally). */
function clueTokens(clue: string): string[] {
  return (clue.toUpperCase().match(/[A-Z]+/g) ?? []).filter((t) => t.length >= 3);
}

/**
 * Scan every clue's words against every grid answer (by stem). Returns
 * QA-shaped findings so the result can feed the existing `clue --revise`
 * machinery directly. The clue's own entry is included too — rule 1 (own
 * clue) and rule 1b (any clue) are the same violation to an editor.
 */
export function findAnswerInClueDups(across: CluedEntry[], down: CluedEntry[]): QAFinding[] {
  const entries = [...across, ...down];
  // stem -> first entry with that answer stem
  const answerStems = new Map<string, CluedEntry>();
  for (const e of entries) {
    if (EXEMPT.has(e.answer)) continue;
    const s = stem(e.answer);
    if (!answerStems.has(s)) answerStems.set(s, e);
  }
  // Answer stems >= 4 letters also match when CONTAINED in a clue word
  // ("basketball" in a clue when BALL is an answer).
  const containable = [...answerStems.entries()].filter(([s]) => s.length >= 4);

  const findings: QAFinding[] = [];
  for (const e of entries) {
    const seen = new Set<string>(); // avoid duplicate findings per clue
    for (const tok of clueTokens(e.clue)) {
      if (EXEMPT.has(tok)) continue;
      const ts = stem(tok);
      let hit = answerStems.get(ts);
      if (!hit) {
        hit = containable.find(([s]) => tok.includes(s))?.[1];
      }
      if (!hit) continue;
      const key = stem(hit.answer);
      if (seen.has(key)) continue;
      seen.add(key);
      findings.push({
        location: `${e.num}${e.dir}`,
        severity: "high",
        category: "duplicate",
        issue: `Clue for ${e.num}${e.dir} (${e.answer}) contains "${tok}" — a form of the answer ${hit.answer} (${hit.num}${hit.dir}). No grid answer may appear in any clue.`,
        suggestion: `Rewrite the ${e.num}${e.dir} clue without any form of "${hit.answer}".`,
      });
    }
  }
  return findings;
}
