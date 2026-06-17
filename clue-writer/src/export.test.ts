import { test } from "node:test";
import assert from "node:assert/strict";
import type { CluedEntry, CluedPuzzle } from "./types.js";
import { resolveMeta } from "./exportShared.js";
import { toPuz } from "./puz.js";
import { toIpuzDoc } from "./ipuz.js";
import { toPdf } from "./pdf.js";

// A tiny, fully-checkable 3x3 puzzle:
//   C A T
//   A R E
//   T E N
function entry(num: number, dir: "A" | "D", row: number, col: number, answer: string, clue: string): CluedEntry {
  return { num, dir, row, col, len: answer.length, answer, score: 50, theme: false, clue };
}

const PUZZLE: CluedPuzzle = {
  day: "Monday",
  difficulty: "Easy",
  themed: false,
  themes: [],
  blocks: 0,
  template: ["...", "...", "..."],
  fill: ["CAT", "ARE", "TEN"],
  source: { wordlist: "test", grid_id: 0 },
  across: [
    entry(1, "A", 0, 0, "CAT", "Feline pet"),
    entry(4, "A", 1, 0, "ARE", "Exist, plurally"),
    entry(5, "A", 2, 0, "TEN", "Perfect score"),
  ],
  down: [
    entry(1, "D", 0, 0, "CAT", "Lion or tiger"),
    entry(2, "D", 0, 1, "ARE", "Form of 'to be'"),
    entry(3, "D", 0, 2, "TEN", "Digits count"),
  ],
};

const META = resolveMeta(PUZZLE, {});

// Independent reader-side checksum (the canonical .puz algorithm).
function cksum(data: Buffer, seed: number): number {
  let c = seed & 0xffff;
  for (const b of data) {
    c = c & 1 ? (c >>> 1) | 0x8000 : c >>> 1;
    c = (c + b) & 0xffff;
  }
  return c;
}

test("toPuz: header, dimensions, and solution decode correctly", () => {
  const buf = toPuz(PUZZLE, META);
  assert.equal(buf.toString("latin1", 0x02, 0x0e), "ACROSS&DOWN\0");
  assert.equal(buf.toString("latin1", 0x18, 0x1c), "1.3\0");
  assert.equal(buf[0x2c], 3, "width");
  assert.equal(buf[0x2d], 3, "height");
  assert.equal(buf.readUInt16LE(0x2e), 6, "clue count (3A + 3D)");

  const sol = buf.toString("latin1", 52, 52 + 9);
  assert.equal(sol, "CATARETEN");
  const grid = buf.toString("latin1", 52 + 9, 52 + 18);
  assert.equal(grid, "---------", "empty player grid");
});

test("toPuz: all checksums validate (re-derived from the bytes)", () => {
  const buf = toPuz(PUZZLE, META);
  const w = buf[0x2c]!;
  const h = buf[0x2d]!;
  const n = buf.readUInt16LE(0x2e);

  const cib = buf.subarray(0x2c, 0x2c + 8);
  const sol = buf.subarray(52, 52 + w * h);
  const grid = buf.subarray(52 + w * h, 52 + 2 * w * h);

  // Parse the NUL-delimited string section.
  const parts = buf.subarray(52 + 2 * w * h).toString("latin1").split("\0");
  parts.pop(); // trailing empty after final NUL
  assert.equal(parts.length, 3 + n + 1, "title, author, copyright, N clues, notes");
  const [title, author, copyright] = parts;
  const clues = parts.slice(3, 3 + n);
  const notes = parts[3 + n]!;

  const cCib = cksum(cib, 0);
  const cSol = cksum(sol, 0);
  const cGrid = cksum(grid, 0);
  const textCksum = (seed: number) => {
    let c = seed;
    for (const s of [title, author, copyright])
      if (s) c = cksum(Buffer.from(s + "\0", "latin1"), c);
    for (const cl of clues) c = cksum(Buffer.from(cl, "latin1"), c);
    if (notes) c = cksum(Buffer.from(notes + "\0", "latin1"), c);
    return c;
  };
  const cPart = textCksum(0);

  // CIB and global checksums in the header.
  assert.equal(buf.readUInt16LE(0x0e), cCib, "CIB checksum");
  let global = cCib;
  global = cksum(sol, global);
  global = cksum(grid, global);
  global = textCksum(global);
  assert.equal(buf.readUInt16LE(0x00), global, "global file checksum");

  // Masked checksums XOR back to the component checksums via "ICHEATED".
  const ICHEATED = Buffer.from("ICHEATED", "latin1");
  const comps = [cCib, cSol, cGrid, cPart];
  for (let i = 0; i < 4; i++) {
    assert.equal(buf[0x10 + i]! ^ ICHEATED[i]!, comps[i]! & 0xff, `masked low byte ${i}`);
    assert.equal(buf[0x14 + i]! ^ ICHEATED[i + 4]!, (comps[i]! >> 8) & 0xff, `masked high byte ${i}`);
  }
});

test("toPuz: clues are ordered by number, across before down", () => {
  const buf = toPuz(PUZZLE, META);
  const parts = buf.subarray(52 + 18).toString("latin1").split("\0");
  parts.pop();
  const clues = parts.slice(3, 3 + 6);
  // nums 1,2,3,4,5 -> 1A,1D,2D,3D,4A,5A
  assert.deepEqual(clues, [
    "Feline pet", // 1A
    "Lion or tiger", // 1D
    "Form of 'to be'", // 2D
    "Digits count", // 3D
    "Exist, plurally", // 4A
    "Perfect score", // 5A
  ]);
});

test("toIpuzDoc: structure, numbering, solution, and clues", () => {
  const doc = toIpuzDoc(PUZZLE, META);
  assert.deepEqual(doc.dimensions, { width: 3, height: 3 });
  assert.deepEqual(doc.kind, ["http://ipuz.org/crossword#1"]);
  // Numbering: top-left = 1, then 2 and 3 across the first row.
  assert.deepEqual(doc.puzzle[0], [1, 2, 3]);
  assert.deepEqual(doc.solution[0], ["C", "A", "T"]);
  assert.deepEqual(doc.solution[2], ["T", "E", "N"]);
  assert.equal(doc.clues.Across.length, 3);
  assert.equal(doc.clues.Down.length, 3);
  assert.deepEqual(doc.clues.Across[0], [1, "Feline pet"]);
  assert.deepEqual(doc.clues.Down[2], [3, "Digits count"]);
});

test("toIpuz: blocks are emitted as '#'", () => {
  const withBlock: CluedPuzzle = {
    ...PUZZLE,
    fill: ["CA#", "ARE", "TEN"],
    template: ["..#", "...", "..."],
  };
  const doc = toIpuzDoc(withBlock, META);
  assert.equal(doc.puzzle[0]![2], "#");
  assert.equal(doc.solution[0]![2], "#");
});

test("toPdf: produces a valid PDF shell containing the title", () => {
  const buf = toPdf(PUZZLE, META);
  assert.equal(buf.toString("latin1", 0, 8), "%PDF-1.4");
  const text = buf.toString("latin1");
  assert.match(text, /\/Type \/Catalog/);
  assert.match(text, /WordFuzz Crossword/); // default title rendered
  assert.match(text, /Feline pet/); // a clue rendered
  assert.match(text, /%%EOF\s*$/);
});

test("export: smart punctuation is folded for legacy formats", () => {
  const fancy: CluedPuzzle = {
    ...PUZZLE,
    across: [
      entry(1, "A", 0, 0, "CAT", "A “fancy” clue — with em dash"),
      ...PUZZLE.across.slice(1),
    ],
  };
  const buf = toPuz(fancy, META);
  const txt = buf.toString("latin1");
  assert.match(txt, /A "fancy" clue - with em dash/);
  assert.doesNotMatch(txt, /—/); // no raw em dash survives
});
