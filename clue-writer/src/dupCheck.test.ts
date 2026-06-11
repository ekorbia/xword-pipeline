// Run: npm test (tsx --test src/dupCheck.test.ts)
import test from "node:test";
import assert from "node:assert/strict";
import { findAnswerInClueDups, stem } from "./dupCheck.js";
import type { CluedEntry } from "./types.js";

function entry(num: number, dir: "A" | "D", answer: string, clue: string): CluedEntry {
  return { num, dir, row: 0, col: 0, len: answer.length, answer, score: 70, theme: false, clue };
}

test("stem mirrors the Rust stemmer", () => {
  assert.equal(stem("TENTH"), "TEN");
  assert.equal(stem("CATS"), "CAT");
  assert.equal(stem("STORIES"), "STORY");
  assert.equal(stem("RUNNING"), "RUN");
  assert.equal(stem("FASTEST"), "FAST");
  assert.equal(stem("OPERA"), "OPERA");
  assert.equal(stem("MASS"), "MASS");
  // Prefixes with the >= 4 remainder guard.
  assert.equal(stem("UNEVENLY"), "EVEN");
  assert.equal(stem("REOPENED"), "OPEN");
  assert.equal(stem("UNION"), "UNION");
  assert.equal(stem("UNIT"), "UNIT");
});

test("prefix forms in clues are flagged (UNEVENLY vs answer EVEN)", () => {
  const across = [entry(19, "A", "EVEN", "Like 2, 4 and 6")];
  const down = [entry(12, "D", "OBOE", "Instrument played unevenly in rehearsal")];
  const findings = findAnswerInClueDups(across, down);
  assert.equal(findings.length, 1);
  assert.equal(findings[0]!.location, "12D");
  assert.match(findings[0]!.issue, /EVEN/);
});

test("answer contained in a clue word is flagged (BALL in 'basketball')", () => {
  const across = [entry(1, "A", "BALL", "Gala")];
  const down = [entry(2, "D", "HOOP", "Basketball target")];
  const findings = findAnswerInClueDups(across, down);
  assert.equal(findings.length, 1);
  assert.equal(findings[0]!.location, "2D");
});

test("flags the user-reported FIRST case: answer appears in other clues", () => {
  const across = [
    entry(34, "A", "TENTH", "First extra inning"),
    entry(52, "A", "LAURA", "First Lady after Hillary"),
  ];
  const down = [entry(7, "D", "FIRST", "Gold-medal position")];
  const findings = findAnswerInClueDups(across, down);
  const locations = findings.map((f) => f.location).sort();
  assert.deepEqual(locations, ["34A", "52A"]);
  for (const f of findings) {
    assert.equal(f.severity, "high");
    assert.equal(f.category, "duplicate");
    assert.match(f.issue, /FIRST/);
  }
});

test("matches by root, not just exact word", () => {
  const across = [entry(1, "A", "RUN", "Jog")];
  const down = [entry(2, "D", "ABC", "Network with 'Running' in a show title")];
  const findings = findAnswerInClueDups(across, down);
  assert.equal(findings.length, 1);
  assert.equal(findings[0]!.location, "2D");
});

test("clean puzzle yields no findings; function words exempt", () => {
  const across = [
    entry(1, "A", "THE", "Definite article"),
    entry(5, "A", "OBOE", "Reed instrument"),
  ];
  const down = [entry(2, "D", "ERA", "Notable period in the past")]; // "the" exempt
  assert.deepEqual(findAnswerInClueDups(across, down), []);
});

test("blanks and short tokens are ignored", () => {
  const across = [entry(1, "A", "NULL", "___ and void")];
  const down = [entry(2, "D", "VOID", "Empty space")];
  // NULL's clue contains "void" -> flagged on 1A against 2D's answer.
  const findings = findAnswerInClueDups(across, down);
  assert.equal(findings.length, 1);
  assert.equal(findings[0]!.location, "1A");
});
