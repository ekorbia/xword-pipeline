// Serialize a CluedPuzzle to ipuz, the modern open JSON puzzle standard
// (http://www.ipuz.org/). ipuz is UTF-8, so unlike .puz we keep clue text
// verbatim. Supported by Crossword Nexus, Exolve, Puzzlme, and others.

import type { CluedPuzzle } from "./types.js";
import {
  dims,
  isBlock,
  letterAt,
  numberMap,
  sortedAcross,
  sortedDown,
  type ExportMeta,
} from "./exportShared.js";

const BLOCK = "#";

/** A cell in the ipuz `puzzle` array: "#" for a block, the clue number for a
 * numbered cell, or 0 for an unnumbered white cell. */
type PuzzleCell = string | number;

export interface IpuzDoc {
  version: string;
  kind: string[];
  title: string;
  author: string;
  copyright: string;
  intro?: string;
  dimensions: { width: number; height: number };
  block: string;
  empty: string;
  puzzle: PuzzleCell[][];
  solution: string[][];
  clues: {
    Across: [number, string][];
    Down: [number, string][];
  };
}

/** Build the ipuz document object for a clued puzzle. */
export function toIpuzDoc(puzzle: CluedPuzzle, meta: ExportMeta): IpuzDoc {
  const { rows, cols } = dims(puzzle);
  const nums = numberMap(puzzle);

  const puzzleGrid: PuzzleCell[][] = [];
  const solutionGrid: string[][] = [];
  for (let r = 0; r < rows; r++) {
    const prow: PuzzleCell[] = [];
    const srow: string[] = [];
    for (let cc = 0; cc < cols; cc++) {
      if (isBlock(puzzle, r, cc)) {
        prow.push(BLOCK);
        srow.push(BLOCK);
      } else {
        prow.push(nums.get(`${r},${cc}`) ?? 0);
        srow.push(letterAt(puzzle, r, cc));
      }
    }
    puzzleGrid.push(prow);
    solutionGrid.push(srow);
  }

  return {
    version: "http://ipuz.org/v2",
    kind: ["http://ipuz.org/crossword#1"],
    title: meta.title,
    author: meta.author,
    copyright: meta.copyright,
    ...(meta.notes ? { intro: meta.notes } : {}),
    dimensions: { width: cols, height: rows },
    block: BLOCK,
    empty: "0",
    puzzle: puzzleGrid,
    solution: solutionGrid,
    clues: {
      Across: sortedAcross(puzzle).map((e) => [e.num, e.clue] as [number, string]),
      Down: sortedDown(puzzle).map((e) => [e.num, e.clue] as [number, string]),
    },
  };
}

/** ipuz file contents (pretty-printed JSON). */
export function toIpuz(puzzle: CluedPuzzle, meta: ExportMeta): string {
  return JSON.stringify(toIpuzDoc(puzzle, meta), null, 2);
}
