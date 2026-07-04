// Run: npm test (tsx --test src/styleGuide.test.ts)
import test from "node:test";
import assert from "node:assert/strict";
import {
  DAY_RUBRIC,
  SIZE_GUIDANCE,
  defaultDay,
  normalizeDay,
  sizeClassOf,
  sizeLine,
} from "./styleGuide.js";

test("sizeClassOf buckets: mini ≤7, midi 8-12, full 13+", () => {
  assert.equal(sizeClassOf(5), "mini");
  assert.equal(sizeClassOf(7), "mini");
  assert.equal(sizeClassOf(8), "midi");
  assert.equal(sizeClassOf(12), "midi");
  assert.equal(sizeClassOf(13), "full");
  assert.equal(sizeClassOf(15), "full");
});

test("defaultDay is size-aware for themeless grids", () => {
  assert.equal(defaultDay(true), "Wednesday"); // themed wins regardless of size
  assert.equal(defaultDay(true, 5), "Wednesday");
  assert.equal(defaultDay(false, 5), "Monday");
  assert.equal(defaultDay(false, 10), "Wednesday");
  assert.equal(defaultDay(false, 15), "Saturday");
  assert.equal(defaultDay(false), "Saturday"); // legacy no-size call = full-size behavior
});

test("sizeLine names the class", () => {
  assert.equal(sizeLine(5, 5), "Grid: 5×5 — MINI class.");
  assert.equal(sizeLine(10, 10), "Grid: 10×10 — MIDI class.");
  assert.equal(sizeLine(15, 15), "Grid: 15×15 — FULL class.");
});

test("normalizeDay accepts day names and friendly words", () => {
  assert.equal(normalizeDay("Wednesday"), "Wednesday");
  assert.equal(normalizeDay("expert"), "Saturday");
  assert.equal(normalizeDay("bogus"), undefined);
});

test("DAY_RUBRIC covers all six days; SIZE_GUIDANCE covers all three classes", () => {
  for (const d of ["MONDAY", "TUESDAY", "WEDNESDAY", "THURSDAY", "FRIDAY", "SATURDAY"]) {
    assert.ok(DAY_RUBRIC.includes(d), `rubric missing ${d}`);
  }
  for (const c of ["MINI", "MIDI", "FULL"]) {
    assert.ok(SIZE_GUIDANCE.includes(c), `size guidance missing ${c}`);
  }
});
