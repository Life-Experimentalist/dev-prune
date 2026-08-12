# What this changes

<!-- One paragraph. What behaviour is different after this PR, from a user's point of view? -->

Closes #

## Why

<!-- The problem. If it fixes a bug, what did the old code do wrong? -->

## Checks

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test --all`
- [ ] Docs updated where behaviour changed (`docs/`, `README.md`, `CHANGELOG.md`)
- [ ] No documentation claim added that the code does not actually do

## Safety

dev-prune's central promise is that it never deletes anything it cannot prove is
recoverable. Tick whichever applies:

- [ ] This PR does not touch deletion, lockfile verification, or idle detection
- [ ] It does, and the invariants in `docs/SAFETY_INVARIANTS.md` still hold — explain below

<!-- If you touched the safety path, describe what you changed and how you tested it. -->

## Notes for the reviewer

<!-- Anything worth knowing: a tradeoff you made, a case you deliberately did not handle. -->
