// Render a CluedPuzzle to a print-ready PDF: a header + empty grid (page 1),
// the clues in a paginated 3-column flow, and a solution grid at the end.
//
// We hand-roll a minimal PDF writer rather than pull in a dependency — a
// crossword page is just rectangles and text, and this keeps clue-writer's
// dependency footprint tiny (it matches the project's lean ethos). Text uses
// the built-in Helvetica family with WinAnsiEncoding; clue wrapping is measured
// against the real Helvetica AFM widths below so lines don't overflow columns.

import type { CluedPuzzle } from "./types.js";
import {
  asciiPunct,
  dims,
  isBlock,
  letterAt,
  numberMap,
  sortedAcross,
  sortedDown,
  type ExportMeta,
} from "./exportShared.js";

// Helvetica glyph advance widths (1/1000 em) for ASCII codes 32..126.
// Digits and '.'/' ' are identical in Helvetica-Bold, so numbers measure the
// same in either weight — which is all we measure in bold.
// prettier-ignore
const HELV_W = [
  278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
  556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556,
  1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
  667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556,
  333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
  556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

function charWidth(code: number): number {
  return code >= 32 && code <= 126 ? HELV_W[code - 32]! : 556;
}

/** Width of a string in points at the given font size. */
function measure(s: string, size: number): number {
  const folded = asciiPunct(s);
  let w = 0;
  for (let i = 0; i < folded.length; i++) w += charWidth(folded.charCodeAt(i));
  return (w / 1000) * size;
}

/** Greedy word-wrap to a max width; hard-breaks any single word that's too long. */
function wrap(text: string, size: number, maxWidth: number): string[] {
  const words = asciiPunct(text).split(/\s+/).filter(Boolean);
  const lines: string[] = [];
  let line = "";
  for (const word of words) {
    const trial = line ? `${line} ${word}` : word;
    if (measure(trial, size) <= maxWidth || !line) {
      if (measure(trial, size) <= maxWidth) {
        line = trial;
        continue;
      }
      // Single word longer than the column: hard-break by characters.
      let chunk = "";
      for (const ch of word) {
        if (measure(chunk + ch, size) > maxWidth && chunk) {
          lines.push(chunk);
          chunk = ch;
        } else {
          chunk += ch;
        }
      }
      line = chunk;
    } else {
      lines.push(line);
      line = word;
    }
  }
  if (line) lines.push(line);
  return lines;
}

/** Escape a string for a PDF literal-string operand and fold smart punctuation. */
function esc(s: string): string {
  return asciiPunct(s).replace(/\\/g, "\\\\").replace(/\(/g, "\\(").replace(/\)/g, "\\)");
}

// ---- page geometry (US Letter) ----
const PAGE_W = 612;
const PAGE_H = 792;
const MARGIN = 54;
const CONTENT_TOP = MARGIN; // top-down y of the first usable line
const CONTENT_BOTTOM = PAGE_H - MARGIN; // top-down y past which we stop
const N_COLS = 3;
const GUTTER = 18;
const COL_W = (PAGE_W - 2 * MARGIN - (N_COLS - 1) * GUTTER) / N_COLS;
const COL_X = Array.from({ length: N_COLS }, (_, i) => MARGIN + i * (COL_W + GUTTER));

const FONT = { reg: "/F1", bold: "/F2" } as const;

/** Accumulates the content-stream operators for one page. */
class Page {
  readonly ops: string[] = [];

  /** Left-anchored text; `baseTop` is the baseline measured from the page top. */
  text(x: number, baseTop: number, size: number, font: string, s: string) {
    const y = PAGE_H - baseTop;
    this.ops.push(`BT ${font} ${size} Tf 0 g ${x.toFixed(2)} ${y.toFixed(2)} Td (${esc(s)}) Tj ET`);
  }

  /** Horizontally centered text around `cx`. */
  textCentered(cx: number, baseTop: number, size: number, font: string, s: string) {
    this.text(cx - measure(s, size) / 2, baseTop, size, font, s);
  }

  rectFill(xLeft: number, yTop: number, w: number, h: number) {
    this.ops.push(`0 g ${xLeft.toFixed(2)} ${(PAGE_H - yTop - h).toFixed(2)} ${w.toFixed(2)} ${h.toFixed(2)} re f`);
  }

  rectStroke(xLeft: number, yTop: number, w: number, h: number, lineW: number) {
    this.ops.push(
      `0 G ${lineW} w ${xLeft.toFixed(2)} ${(PAGE_H - yTop - h).toFixed(2)} ${w.toFixed(2)} ${h.toFixed(2)} re S`,
    );
  }
}

/** Draw the grid (white cells outlined, blocks filled), optionally with letters. */
function drawGrid(page: Page, puzzle: CluedPuzzle, x0: number, y0: number, s: number, letters: boolean) {
  const { rows, cols } = dims(puzzle);
  const nums = numberMap(puzzle);
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const x = x0 + c * s;
      const y = y0 + r * s;
      if (isBlock(puzzle, r, c)) {
        page.rectFill(x, y, s, s);
        continue;
      }
      page.rectStroke(x, y, s, s, 0.75);
      const n = nums.get(`${r},${c}`);
      if (n !== undefined) page.text(x + 1.4, y + s * 0.3 + 0.5, s * 0.28, FONT.reg, String(n));
      if (letters) page.textCentered(x + s / 2, y + s * 0.74, s * 0.62, FONT.bold, letterAt(puzzle, r, c));
    }
  }
  // Heavier outer border.
  page.rectStroke(x0, y0, cols * s, rows * s, 1.5);
}

/** Cell size so the grid fits within `maxW` × `maxH`. */
function cellSize(rows: number, cols: number, maxW: number, maxH: number): number {
  return Math.min(maxW / cols, maxH / rows);
}

export interface PdfOptions {
  /** Append a final page with the filled solution grid (default true). */
  solution?: boolean;
}

/** Render the clued puzzle to PDF bytes. */
export function toPdf(puzzle: CluedPuzzle, meta: ExportMeta, opts: PdfOptions = {}): Buffer {
  const { rows, cols } = dims(puzzle);
  const pages: Page[] = [];
  const newPage = () => {
    const p = new Page();
    pages.push(p);
    return p;
  };

  // ---- page 1: header + empty grid ----
  const p1 = newPage();
  p1.textCentered(PAGE_W / 2, CONTENT_TOP + 18, 18, FONT.bold, meta.title);
  let headerY = CONTENT_TOP + 18 + 16;
  const subtitle = [meta.notes, `by ${meta.author}`].filter(Boolean).join("   ·   ");
  if (subtitle) {
    p1.textCentered(PAGE_W / 2, headerY, 10, FONT.reg, subtitle);
    headerY += 14;
  }

  const gridTop = headerY + 14;
  const s = cellSize(rows, cols, PAGE_W - 2 * MARGIN, 430);
  const gridX = (PAGE_W - cols * s) / 2;
  drawGrid(p1, puzzle, gridX, gridTop, s, false);
  const cluesStartY = gridTop + rows * s + 22;

  // ---- clue flow: ACROSS then DOWN, 3 columns, paginated ----
  const CLUE_SIZE = 9;
  const LINE_H = CLUE_SIZE * 1.18;
  const indent = measure("000.", CLUE_SIZE) + 3;

  // Column cursor. Page 1's columns begin below the grid; later pages use the
  // full height.
  let page = p1;
  let col = 0;
  let y = cluesStartY;
  const colTopForCurrentPage = () => (page === p1 ? cluesStartY : CONTENT_TOP);

  const advanceColumn = () => {
    col++;
    if (col >= N_COLS) {
      page = newPage();
      col = 0;
    }
    y = colTopForCurrentPage();
  };
  const ensure = (h: number) => {
    if (y + h > CONTENT_BOTTOM) advanceColumn();
  };

  const heading = (label: string) => {
    ensure(LINE_H + 6);
    page.text(COL_X[col]!, y + 11, 11, FONT.bold, label);
    y += 18;
  };
  const clue = (num: number, text: string) => {
    const lines = wrap(text, CLUE_SIZE, COL_W - indent);
    const h = lines.length * LINE_H + 4;
    ensure(h);
    const x = COL_X[col]!;
    page.text(x, y + CLUE_SIZE, CLUE_SIZE, FONT.bold, `${num}.`);
    lines.forEach((ln, i) => page.text(x + indent, y + CLUE_SIZE + i * LINE_H, CLUE_SIZE, FONT.reg, ln));
    y += h;
  };

  heading("ACROSS");
  for (const e of sortedAcross(puzzle)) clue(e.num, e.clue);
  // Down list starts in a fresh column so the two lists never interleave.
  advanceColumn();
  heading("DOWN");
  for (const e of sortedDown(puzzle)) clue(e.num, e.clue);

  // ---- solution page ----
  if (opts.solution !== false) {
    const ps = newPage();
    ps.textCentered(PAGE_W / 2, CONTENT_TOP + 16, 16, FONT.bold, "Solution");
    const sg = cellSize(rows, cols, PAGE_W - 2 * MARGIN, 460);
    drawGrid(ps, puzzle, (PAGE_W - cols * sg) / 2, CONTENT_TOP + 40, sg, true);
  }

  return assemble(pages);
}

// ---- low-level PDF assembly ----

/** Serialize the pages into a complete PDF file (objects + xref + trailer). */
function assemble(pages: Page[]): Buffer {
  const chunks: Buffer[] = [];
  const offsets: number[] = []; // byte offset of each object, 1-indexed
  let pos = 0;
  const push = (s: string | Buffer) => {
    const b = typeof s === "string" ? Buffer.from(s, "latin1") : s;
    chunks.push(b);
    pos += b.length;
  };
  const obj = (id: number, body: string | Buffer) => {
    offsets[id] = pos;
    push(`${id} 0 obj\n`);
    push(body);
    push("\nendobj\n");
  };

  push("%PDF-1.4\n");

  // Fixed objects: 1 catalog, 2 pages, 3 Helvetica, 4 Helvetica-Bold.
  // Page objects start at id 5: page i -> 5+2i, its content stream -> 6+2i.
  const pageObjId = (i: number) => 5 + 2 * i;
  const contentObjId = (i: number) => 6 + 2 * i;
  const kids = pages.map((_, i) => `${pageObjId(i)} 0 R`).join(" ");

  obj(1, "<< /Type /Catalog /Pages 2 0 R >>");
  obj(
    2,
    `<< /Type /Pages /Kids [${kids}] /Count ${pages.length} ` +
      `/MediaBox [0 0 ${PAGE_W} ${PAGE_H}] ` +
      `/Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> >>`,
  );
  obj(3, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
  obj(4, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>");

  pages.forEach((p, i) => {
    obj(pageObjId(i), `<< /Type /Page /Parent 2 0 R /Contents ${contentObjId(i)} 0 R >>`);
    const stream = Buffer.from(p.ops.join("\n"), "latin1");
    offsets[contentObjId(i)] = pos;
    push(`${contentObjId(i)} 0 obj\n<< /Length ${stream.length} >>\nstream\n`);
    push(stream);
    push("\nendstream\nendobj\n");
  });

  // Cross-reference table.
  const xrefStart = pos;
  const count = contentObjId(pages.length - 1) + 1; // highest id + 1
  let xref = `xref\n0 ${count}\n0000000000 65535 f \n`;
  for (let id = 1; id < count; id++) {
    xref += `${String(offsets[id] ?? 0).padStart(10, "0")} 00000 n \n`;
  }
  push(xref);
  push(`trailer\n<< /Size ${count} /Root 1 0 R >>\nstartxref\n${xrefStart}\n%%EOF\n`);

  return Buffer.concat(chunks);
}
