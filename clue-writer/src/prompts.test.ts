// Run: npm test (tsx --test src/prompts.test.ts)
//
// Guards the size-awareness wiring: every prompt that states a target day must
// also state the grid's size class, and the QA editor's system prompt must
// carry the same day rubric the clue writer wrote to. This is the regression
// test for "QA judged a 5×5 Wednesday against mid-week 15×15 expectations."
import test from "node:test";
import assert from "node:assert/strict";
import type { CluedEntry, CluedPuzzle, LibraryGrid, QAReport } from "./types.js";
import { buildReviseMessage, buildUserMessage } from "./clueWriter.js";
import { EDITOR_GUIDE, buildReviewMessage } from "./editor.js";
import { STYLE_GUIDE } from "./styleGuide.js";

const FILL_5 = ["SPAM#", "TIARA", "ARENA", "BONES", "#SSTS"];
const TPL_5 = ["....#", ".....", ".....", ".....", "#...."];

function entry(num: number, dir: "A" | "D", answer: string, clue = ""): CluedEntry {
  return { num, dir, row: 0, col: 0, len: answer.length, answer, score: 60, theme: false, clue };
}

const grid5: LibraryGrid = {
  id: 0,
  blocks: 2,
  themed: false,
  mean_score: 60,
  min_score: 50,
  iffy: 0,
  template: TPL_5,
  fill: FILL_5,
  entries: [entry(1, "A", "SPAM"), entry(6, "D", "TIARA")],
};

const puzzle5: CluedPuzzle = {
  day: "Wednesday",
  themed: false,
  themes: [],
  blocks: 2,
  template: TPL_5,
  fill: FILL_5,
  source: { wordlist: "test", grid_id: 0 },
  across: [entry(1, "A", "SPAM", "Canned lunch staple")],
  down: [entry(6, "D", "TIARA", "Pageant topper")],
};

const report: QAReport = {
  verdict: "minor-revisions",
  summary: "test",
  findings: [
    {
      location: "1A",
      severity: "medium",
      category: "difficulty",
      issue: "too easy",
      suggestion: "toughen",
    },
  ],
};

test("clue-writer user message states day guidance and size class", () => {
  const msg = buildUserMessage(grid5, "Wednesday");
  assert.ok(msg.includes("Target day: Wednesday"));
  assert.ok(msg.includes("WEDNESDAY — medium"));
  assert.ok(msg.includes("Grid: 5×5 — MINI class."));
});

test("revise message states day guidance and size class", () => {
  const msg = buildReviseMessage(puzzle5, report);
  assert.ok(msg.includes("Target day: Wednesday"));
  assert.ok(msg.includes("Grid: 5×5 — MINI class."));
});

test("QA review message states day and size class", () => {
  const msg = buildReviewMessage(puzzle5);
  assert.ok(msg.includes("Target day: Wednesday"));
  assert.ok(msg.includes("Grid: 5×5 — MINI class."));
});

test("QA scope: grid omits clues; clue/full keep them; each names its scope", () => {
  // grid scope reviews answers only — no clue text, so its findings apply to
  // every tier written on this grid.
  const grid = buildReviewMessage(puzzle5, "grid");
  assert.ok(grid.includes("GRID-LEVEL ONLY"));
  assert.ok(grid.includes("SPAM"), "grid scope keeps the answers");
  assert.ok(!grid.includes("Canned lunch staple"), "grid scope must drop clue text");
  assert.ok(grid.includes("Grid: 5×5 — MINI class."), "size still stated in grid scope");

  // clue scope carries the clues and is judged against the day.
  const clue = buildReviewMessage(puzzle5, "clue");
  assert.ok(clue.includes("SCOPE — CLUE-LEVEL"));
  assert.ok(clue.includes("Canned lunch staple"), "clue scope keeps clue text");
  assert.ok(clue.includes("Target day: Wednesday"));

  // full scope (the single-tier default) reviews everything.
  const full = buildReviewMessage(puzzle5, "full");
  assert.ok(full.includes("SCOPE — FULL"));
  assert.ok(full.includes("Canned lunch staple"));
  // default arg is "full".
  assert.equal(buildReviewMessage(puzzle5), full);
});

test("QA editor guide embeds the writer's day rubric and the size classes", () => {
  assert.ok(EDITOR_GUIDE.includes("WEDNESDAY — medium"), "day rubric missing from editor guide");
  assert.ok(EDITOR_GUIDE.includes("SATURDAY — the hardest puzzle"));
  assert.ok(EDITOR_GUIDE.includes("MINI (7×7 and under"), "size classes missing from editor guide");
});

test("style guide carries the size classes", () => {
  assert.ok(STYLE_GUIDE.includes("MINI (7×7 and under"));
  assert.ok(STYLE_GUIDE.includes("size class"));
});
