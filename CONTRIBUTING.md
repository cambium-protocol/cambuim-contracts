# Contributing to cambuim-contracts

Thanks for your interest in contributing. This document covers everything you need to get a change from idea to merged PR.

---

## Table of contents

- [Before you start](#before-you-start)
- [What to work on](#what-to-work-on)
- [Development setup](#development-setup)
- [Making changes](#making-changes)
- [Tests](#tests)
- [Code style](#code-style)
- [Opening a pull request](#opening-a-pull-request)
- [Review process](#review-process)
- [Security vulnerabilities](#security-vulnerabilities)

---

## Before you start

- Check [open issues](https://github.com/cambium-protocol/cambuim-contracts/issues) and [open PRs](https://github.com/cambium-protocol/cambuim-contracts/pulls) before starting work. If you want to tackle something substantial, open an issue first to discuss the approach — this avoids duplicated effort and design disagreements late in review.
- Read the [README](./README.md), especially the [Trust assumptions](./README.md#trust-assumptions) and [Roadmap](./README.md#roadmap) sections. The roadmap lists what is deferred and why; the trust assumptions explain the highest-value attack surfaces.
- For security-sensitive findings (auth paths, verifying-key governance, ZK verifier), see [SECURITY.md](./SECURITY.md) and report privately rather than opening a public issue.

---

## What to work on

Good first contributions:

- Improving test coverage — we target >90% branch coverage on `registry`, `credit-token`, and `retirement`, and 100% on any auth/access-control path
- Documentation fixes and clarifications
- Adding missing doc comments to public contract functions

Larger contributions that are explicitly deferred and ready for work (see [Roadmap](./README.md#roadmap)):

- Limit order book in `marketplace` (currently `Error::NotYetImplemented`)
- Shielded retirement in `retirement` with `shield=true` (currently `Error::NotYetImplemented`)

**Do not work on these without prior discussion:**

- Multi-sig + timelock governance in `registry/src/governance.rs` — this is the highest-value attack surface and must be designed carefully before implementation
- Real Groth16/BN254 verification in `zk-verifier` — depends on `zk-circuits` publishing verifying keys; coordinate with that repo first

---

## Development setup

Prerequisites:

- Rust stable (1.79+)
- `wasm32v1-none` target: `rustup target add wasm32v1-none`
- `stellar-cli` v27+: `cargo install --locked stellar-cli`

Clone and build:

```bash
git clone https://github.com/cambium-protocol/cambuim-contracts.git
cd cambuim-contracts
cargo build
```

Build WASM artifacts:

```bash
stellar contract build
```

> Use `stellar contract build`, not `cargo build --target wasm32-unknown-unknown`. The latter targets the wrong WASM variant and the Soroban VM will reject it at deploy time.

---

## Making changes

1. Fork the repo and create a branch off `main`:
   ```bash
   git checkout -b your-github-username/short-description
   ```
2. Keep changes focused. One logical change per PR makes review faster and keeps git history useful.
3. If you're adding a new contract feature, add or update the corresponding entry in the public interface table in README.md.
4. If your change affects trust assumptions or auth paths, update [SECURITY.md](./SECURITY.md) and call it out explicitly in your PR description.

---

## Tests

Write tests for any new contract logic. The test targets are:

```bash
# Unit tests for a single contract
cargo test -p registry
cargo test -p credit-token
cargo test -p zk-verifier
cargo test -p marketplace
cargo test -p retirement

# Full workspace
cargo test --workspace

# Integration tests (multi-contract flows in the Soroban local sandbox)
cargo test -p integration-tests
```

Coverage targets:

- >90% branch coverage on `registry`, `credit-token`, and `retirement`
- 100% coverage on any authorization or access-control path

Check coverage locally:

```bash
cargo tarpaulin --workspace --out Html
```

PRs that reduce coverage on auth paths will not be merged.

---

## Code style

Before pushing:

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
```

CI enforces both. A PR with fmt or clippy failures will not be reviewed until they are fixed.

A few conventions used in this codebase:

- All token arithmetic uses checked operations (`checked_add`, `checked_mul`, etc.); overflow returns `Error::Overflow`, never panics or wraps silently.
- Cross-contract calls follow checks-effects-interactions ordering even where classic EVM reentrancy is not possible.
- Public contract functions have doc comments explaining preconditions, what the function does, and what errors it can return.
- Error variants are defined per-contract in the contract's `types.rs`; do not return raw strings or panics from contract code.

---

## Opening a pull request

- Target `main`.
- PR title: short imperative sentence, under 70 characters (e.g. `Add limit order cancellation to marketplace`).
- PR description should cover:
  - What the change does and why
  - What you tested and how
  - Any deferred follow-up work or known limitations
  - Whether any trust assumptions or auth paths are affected
- Link the related issue if one exists (`Closes #123`).

---

## Review process

- A maintainer will review within a few days. If you haven't heard back in a week, leave a comment on the PR.
- Expect at least one round of feedback on anything touching auth paths or cross-contract interactions.
- Once approved, a maintainer will merge. We use squash-merge for small changes and merge commits for larger features to preserve meaningful history.

---

## Security vulnerabilities

Do **not** open a public GitHub issue for security-sensitive findings. Report them privately per [SECURITY.md](./SECURITY.md).
