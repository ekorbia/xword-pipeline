// Central model registry — one place to see (and change) which Claude model
// each pipeline step uses, and why.
//
// Step-by-step reasoning:
//   - clue / revise: wordplay sophistication IS the product (especially the
//     Hard/Expert tiers); Opus earns its cost here.
//   - qa: the publication gate. False negatives directly degrade puzzles, and
//     its token volume is small — downgrading saves pennies and risks a lot.
//   - themeIdea: was Sonnet 4.6 briefly (cheaper, quality fine) but its long
//     deliberation on the letter-counting checks made runs slow/truncation-
//     prone; Opus 4.8 (same price as 4.7, current flagship) reasons more
//     efficiently here. Flip back to claude-sonnet-4-6 if cost matters more
//     than latency.
//   - explain: short, factual, post-solve recaps. Haiku handles these well at
//     ~1/5 the cost; the explain CLI accepts --model to override per run.
//   - gradeWords: recognizability judgments over tens of thousands of words —
//     world knowledge, not deep reasoning, and volume makes cost matter.
//     Sonnet is the sweet spot; bump to Opus via --model for a quality pass.
export const MODELS = {
  clue: "claude-opus-4-7",
  qa: "claude-opus-4-7",
  themeIdea: "claude-opus-4-8",
  explain: "claude-haiku-4-5",
  gradeWords: "claude-sonnet-4-6",
} as const;
