// Central model registry — one place to see (and change) which Claude model
// each pipeline step uses, and why.
//
// Step-by-step reasoning:
//   - clue / revise: wordplay sophistication IS the product (especially the
//     Hard/Expert tiers); Opus earns its cost here.
//   - qa: the publication gate. False negatives directly degrade puzzles, and
//     its token volume is small — downgrading saves pennies and risks a lot.
//   - themeIdea: brainstorming with a human filter at the end — the user picks
//     from candidate sets, so Sonnet's quality ceiling is plenty and it's ~40%
//     of Opus's price.
//   - explain: short, factual, post-solve recaps. Haiku handles these well at
//     ~1/5 the cost; the explain CLI accepts --model to override per run.
export const MODELS = {
  clue: "claude-opus-4-7",
  qa: "claude-opus-4-7",
  themeIdea: "claude-sonnet-4-6",
  explain: "claude-haiku-4-5",
} as const;
