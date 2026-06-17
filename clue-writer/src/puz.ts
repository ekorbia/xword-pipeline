// Serialize a CluedPuzzle to the AcrossLite .puz binary format (v1.3).
//
// The .puz format is the lingua franca of the crossword world — AcrossLite, Puz,
// xword, Crossword Solver, and most mobile apps import it. The fiddly part is the
// checksum scheme; see https://github.com/alexdej/puzpy and the community
// fileformat notes. We implement it exactly so produced files validate cleanly.

import type { CluedPuzzle } from "./types.js";
import {
  dims,
  isBlock,
  letterAt,
  orderedClues,
  toLatin1,
  type ExportMeta,
} from "./exportShared.js";

const MAGIC = "ACROSS&DOWN\0";
const VERSION = "1.3\0";
const ICHEATED = [0x49, 0x43, 0x48, 0x45, 0x41, 0x54, 0x45, 0x44]; // "ICHEATED"

/** The .puz running 16-bit checksum, folded over a byte region. */
function cksumRegion(data: Buffer, seed: number): number {
  let c = seed & 0xffff;
  for (const b of data) {
    c = c & 1 ? (c >>> 1) | 0x8000 : c >>> 1;
    c = (c + b) & 0xffff;
  }
  return c;
}

/** Checksum over the text section (titles include their NUL; clues do not). */
function cksumText(meta: ExportMeta, clues: Buffer[], seed: number): number {
  let c = seed & 0xffff;
  const withNul = (s: string) => (s ? cksumRegion(Buffer.concat([toLatin1(s), Buffer.from([0])]), c) : c);
  c = withNul(meta.title);
  c = withNul(meta.author);
  c = withNul(meta.copyright);
  for (const clue of clues) c = cksumRegion(clue, c); // clues: no trailing NUL
  c = withNul(meta.notes); // notes only count for >= v1.3, which we emit
  return c;
}

/** A NUL-terminated Latin-1 string buffer. */
function cstr(s: string): Buffer {
  return Buffer.concat([toLatin1(s), Buffer.from([0])]);
}

/**
 * Build a .puz file for the given clued puzzle. Returns the raw bytes.
 */
export function toPuz(puzzle: CluedPuzzle, meta: ExportMeta): Buffer {
  const { rows, cols } = dims(puzzle);

  // Solution grid (letters, '.' for blocks) and the empty player grid
  // ('-' for white, '.' for black), both row-major.
  const sol = Buffer.alloc(rows * cols);
  const grid = Buffer.alloc(rows * cols);
  for (let r = 0; r < rows; r++) {
    for (let cc = 0; cc < cols; cc++) {
      const i = r * cols + cc;
      if (isBlock(puzzle, r, cc)) {
        sol[i] = 0x2e; // '.'
        grid[i] = 0x2e; // '.'
      } else {
        sol[i] = letterAt(puzzle, r, cc).charCodeAt(0);
        grid[i] = 0x2d; // '-'
      }
    }
  }

  const clueStrings = orderedClues(puzzle);
  const clueBufs = clueStrings.map((s) => toLatin1(s));
  const numClues = clueStrings.length;

  // The CIB block: the 8 bytes at header offset 0x2C (width, height, #clues,
  // puzzle-type bitmask, scrambled tag), all little-endian.
  const cib = Buffer.alloc(8);
  cib.writeUInt8(cols, 0);
  cib.writeUInt8(rows, 1);
  cib.writeUInt16LE(numClues, 2);
  cib.writeUInt16LE(0x0001, 4); // bitmask: normal puzzle
  cib.writeUInt16LE(0x0000, 6); // scrambled tag: unscrambled

  // Component checksums.
  const cCib = cksumRegion(cib, 0);
  const cSol = cksumRegion(sol, 0);
  const cGrid = cksumRegion(grid, 0);
  const cPart = cksumText(meta, clueBufs, 0);

  // Global checksum: CIB, then solution, grid, and text accumulated in order.
  let cGlobal = cCib;
  cGlobal = cksumRegion(sol, cGlobal);
  cGlobal = cksumRegion(grid, cGlobal);
  cGlobal = cksumText(meta, clueBufs, cGlobal);

  // Masked checksums: low byte then high byte of each component, XOR "ICHEATED".
  const masked = Buffer.from([
    ICHEATED[0]! ^ (cCib & 0xff),
    ICHEATED[1]! ^ (cSol & 0xff),
    ICHEATED[2]! ^ (cGrid & 0xff),
    ICHEATED[3]! ^ (cPart & 0xff),
    ICHEATED[4]! ^ ((cCib >> 8) & 0xff),
    ICHEATED[5]! ^ ((cSol >> 8) & 0xff),
    ICHEATED[6]! ^ ((cGrid >> 8) & 0xff),
    ICHEATED[7]! ^ ((cPart >> 8) & 0xff),
  ]);

  // 52-byte header.
  const header = Buffer.alloc(52);
  header.writeUInt16LE(cGlobal, 0x00); // overall file checksum
  header.write(MAGIC, 0x02, "latin1"); // 12-byte magic incl. NUL
  header.writeUInt16LE(cCib, 0x0e); // CIB checksum
  masked.copy(header, 0x10); // 8 masked checksum bytes
  header.write(VERSION, 0x18, "latin1"); // "1.3\0"
  // 0x1C reserved (2), 0x1E scrambled checksum (2), 0x20 reserved (12) — left zero.
  header.writeUInt8(cols, 0x2c);
  header.writeUInt8(rows, 0x2d);
  header.writeUInt16LE(numClues, 0x2e);
  header.writeUInt16LE(0x0001, 0x30); // bitmask
  header.writeUInt16LE(0x0000, 0x32); // scrambled tag

  // String section: title, author, copyright, clues (in order), notes.
  const strings = Buffer.concat([
    cstr(meta.title),
    cstr(meta.author),
    cstr(meta.copyright),
    ...clueBufs.map((b) => Buffer.concat([b, Buffer.from([0])])),
    cstr(meta.notes),
  ]);

  return Buffer.concat([header, sol, grid, strings]);
}
