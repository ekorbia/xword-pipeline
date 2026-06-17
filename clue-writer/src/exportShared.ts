// Shared helpers for the .puz / .ipuz / PDF exporters. A CluedPuzzle is the
// finished product (filled grid + one clue per answer); these helpers turn it
// into the geometry the file formats want: dimensions, a per-cell numbering
// map, ordered clue lists, and sanitized text.

import type { CluedEntry, CluedPuzzle } from "./types.js";

/** Publication metadata stamped into every export. */
export interface ExportMeta {
  title: string;
  author: string;
  copyright: string;
  /** Free-form note (difficulty / theme); shown in .puz notes, ipuz "intro". */
  notes: string;
}

/** Fill in sensible defaults for any metadata the caller didn't supply. */
export function resolveMeta(puzzle: CluedPuzzle, partial: Partial<ExportMeta>): ExportMeta {
  const diff = puzzle.difficulty ?? puzzle.day;
  const themeNote = puzzle.themed && puzzle.themes.length ? ` · Theme: ${puzzle.themes.join(", ")}` : "";
  return {
    title: partial.title ?? "WordFuzz Crossword",
    author: partial.author ?? "WordFuzz",
    copyright: partial.copyright ?? "© WordFuzz",
    notes: partial.notes ?? `Difficulty: ${diff}${themeNote}`,
  };
}

export interface Dims {
  rows: number;
  cols: number;
}

/** Grid dimensions, validated to be a non-empty rectangle. */
export function dims(puzzle: CluedPuzzle): Dims {
  const rows = puzzle.fill.length;
  if (rows === 0) throw new Error("puzzle has no rows");
  const cols = puzzle.fill[0]!.length;
  if (cols === 0) throw new Error("puzzle has no columns");
  for (let r = 0; r < rows; r++) {
    if (puzzle.fill[r]!.length !== cols) {
      throw new Error(`row ${r} has width ${puzzle.fill[r]!.length}, expected ${cols}`);
    }
  }
  return { rows, cols };
}

/** A black square in the solved fill is marked with '#'. */
export function isBlock(puzzle: CluedPuzzle, row: number, col: number): boolean {
  return puzzle.fill[row]![col] === "#";
}

/** The (uppercase) solution letter at a white cell. */
export function letterAt(puzzle: CluedPuzzle, row: number, col: number): string {
  return puzzle.fill[row]![col]!.toUpperCase();
}

/**
 * Map "row,col" -> clue number for every numbered cell. A cell is numbered if
 * it begins an across or down entry; the two share the same number, so building
 * from both entry lists is consistent.
 */
export function numberMap(puzzle: CluedPuzzle): Map<string, number> {
  const m = new Map<string, number>();
  for (const e of [...puzzle.across, ...puzzle.down]) {
    m.set(`${e.row},${e.col}`, e.num);
  }
  return m;
}

/** Across entries, then down entries, each sorted by clue number. */
export function sortedAcross(puzzle: CluedPuzzle): CluedEntry[] {
  return [...puzzle.across].sort((a, b) => a.num - b.num);
}
export function sortedDown(puzzle: CluedPuzzle): CluedEntry[] {
  return [...puzzle.down].sort((a, b) => a.num - b.num);
}

/**
 * Clue strings in .puz order: by increasing cell number, across before down at
 * the same number. This matches the row-major, across-first numbering AcrossLite
 * expects.
 */
export function orderedClues(puzzle: CluedPuzzle): string[] {
  const acrossByNum = new Map(puzzle.across.map((e) => [e.num, e]));
  const downByNum = new Map(puzzle.down.map((e) => [e.num, e]));
  const nums = [...new Set([...acrossByNum.keys(), ...downByNum.keys()])].sort((a, b) => a - b);
  const out: string[] = [];
  for (const n of nums) {
    const a = acrossByNum.get(n);
    if (a) out.push(a.clue);
    const d = downByNum.get(n);
    if (d) out.push(d.clue);
  }
  return out;
}

// ---- text sanitization ----

/**
 * Replace the "smart" Unicode punctuation Claude tends to emit with ASCII
 * equivalents. Keeps clues legible in legacy (.puz / Latin-1) and PDF contexts
 * where the full Unicode range isn't available.
 */
export function asciiPunct(s: string): string {
  return s
    .replace(/[‘’‚′]/g, "'") // ' ' ‚ ′ -> '
    .replace(/[“”„″]/g, '"') // " " „ ″ -> "
    .replace(/[–—‒―]/g, "-") // – — ‒ ― -> -
    .replace(/…/g, "...") // … -> ...
    .replace(/•/g, "*") // bullet -> * (· U+00B7 is kept; it exists in Latin-1/WinAnsi)
    .replace(/ /g, " "); // nbsp -> space
}

/**
 * Encode a string to Latin-1 bytes for the .puz format. Smart punctuation is
 * folded to ASCII first; any remaining codepoint above 0xFF becomes '?'.
 */
export function toLatin1(s: string): Buffer {
  const folded = asciiPunct(s);
  const bytes = Buffer.alloc(folded.length);
  for (let i = 0; i < folded.length; i++) {
    const code = folded.charCodeAt(i);
    bytes[i] = code <= 0xff ? code : 0x3f; // '?'
  }
  return bytes;
}
