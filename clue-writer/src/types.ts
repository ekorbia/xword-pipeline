// Types mirroring the xfill grid-library JSON (emitted by the `library` and
// `theme` Rust binaries). Shared shape for the clue-writing layer; intended to
// be reused by the eventual play-app frontend.

export type Direction = "A" | "D";

export interface LibraryEntry {
  num: number;
  dir: Direction;
  row: number;
  col: number;
  len: number;
  answer: string;
  score: number;
  theme: boolean;
}

export interface LibraryGrid {
  id: number;
  blocks: number;
  themed: boolean;
  mean_score: number;
  min_score: number;
  iffy: number;
  template: string[];
  fill: string[];
  entries: LibraryEntry[];
}

export interface LibraryFile {
  wordlist: string;
  target_blocks: number;
  themed: boolean;
  themes: string[];
  count: number;
  grids: LibraryGrid[];
}

// ---- Clue-writing output ----

export type Day = "Monday" | "Tuesday" | "Wednesday" | "Thursday" | "Friday" | "Saturday";

export interface CluedEntry extends LibraryEntry {
  clue: string;
}

export interface CluedPuzzle {
  day: Day;
  /** Friendly difficulty word derived from `day` (e.g. "Expert"). Display-only. */
  difficulty?: string;
  themed: boolean;
  themes: string[];
  blocks: number;
  template: string[];
  fill: string[];
  source: { wordlist: string; grid_id: number };
  across: CluedEntry[];
  down: CluedEntry[];
}

// ---- Editorial QA ----

export type Severity = "high" | "medium" | "low";
export type QACategory =
  | "fill"
  | "clue-accuracy"
  | "duplicate"
  | "difficulty"
  | "fairness"
  | "breakfast-test"
  | "theme"
  | "style";

export interface QAFinding {
  location: string; // e.g. "17A", "23D × 8A crossing", or "puzzle-wide"
  severity: Severity;
  category: QACategory;
  issue: string;
  suggestion: string;
}

export interface QAReport {
  verdict: "ready" | "minor-revisions" | "needs-work";
  summary: string;
  findings: QAFinding[];
}

// ---- Theme ideation ----

export interface ThemeAnswer {
  text: string; // letters only, no spaces (e.g. WAITINGFORGODOT)
  len: number;
  note: string; // why this fits the theme
}

export interface ThemeIdea {
  concept: string;
  revealer: string; // "" if none
  answers: ThemeAnswer[];
  rationale: string;
}

export interface ThemeIdeas {
  themes: ThemeIdea[];
}
