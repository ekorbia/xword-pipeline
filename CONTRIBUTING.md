# Contributing to xword-pipeline

Thanks for your interest in contributing! This project has two layers and the
contribution flow differs a bit between them.

## Layout

- **`fill-engine/`** — Rust workspace. The CSP fill engine, grid generator, and
  curated-library tooling. Runs offline, no API key needed.
- **`clue-writer/`** — TypeScript. Calls Claude (`@anthropic-ai/sdk`) for clue
  writing, editorial QA, post-solve explanations, and theme ideation.

Each component has its own `README.md` covering build/run details.

## Getting set up

```bash
# Rust engine
(cd fill-engine && cargo build --release)

# Claude clue layer (requires Node 22+)
(cd clue-writer && npm install)

# For the Claude steps:
export ANTHROPIC_API_KEY=sk-ant-...
```

## Running tests locally

```bash
# Rust unit tests (fast)
(cd fill-engine && cargo test --release)

# TypeScript typecheck for the Claude layer
(cd clue-writer && npm run typecheck)
```

CI runs both on every push and PR; please make sure your branch is green
locally before opening a PR.

## What to expect in a good PR

- **One clear change per PR.** Refactor + feature + doc all in one branch is
  hard to review; split them.
- **Tests.** New behavior in `fill-engine` should land with a Rust unit test;
  prompt or schema changes in `clue-writer` should land with a `--dry-run`
  output you've verified locally.
- **No `ANTHROPIC_API_KEY` in commits or CI.** The Claude tools all support
  `--dry-run` for prompt previews without spending tokens.
- **Cargo formatting.** `cargo fmt` and `cargo clippy --workspace -- -D warnings`
  before pushing helps keep the diff focused on the actual change.

## Filing an issue

Use the bug-report or feature-request template. For bug reports, include the
exact command, the relevant snippets of the JSON artifacts under `out/`, and
the Rust/Node versions you're running.

## Cost note for AI-related changes

The Claude steps cost real money (a few cents per call on Claude Opus 4.7). If
you're iterating on a prompt or pipeline change, always test with `--dry-run`
first; only spend tokens to verify behavior at the end.

## License

By contributing you agree that your contributions will be licensed under the
project's [MIT license](./LICENSE).
