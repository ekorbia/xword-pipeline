<!-- Thanks for the PR! Please make sure it covers ONE focused change. -->

## Summary
<!-- What does this change, and why? -->

## Component(s)
- [ ] fill-engine (Rust)
- [ ] clue-writer (TypeScript / Claude)
- [ ] run-pipeline.sh / orchestration
- [ ] docs only

## How was this tested?
<!-- For fill-engine: which cargo tests cover this? For clue-writer: what does `--dry-run` show? -->

## Cost note (Claude-related changes only)
<!-- Did you measure the change to per-puzzle token usage? Did caching still hit? -->

## Checklist
- [ ] `cargo fmt --check` passes (fill-engine)
- [ ] `cargo clippy --workspace -- -D warnings` passes (fill-engine)
- [ ] `cargo test --workspace --release` passes (fill-engine)
- [ ] `npm run typecheck` passes (clue-writer)
- [ ] No `ANTHROPIC_API_KEY` or other secrets are committed
- [ ] Docs updated (README and/or component READMEs) if user-facing behavior changed
