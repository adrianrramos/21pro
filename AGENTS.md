# Agent instructions

## Scope

These instructions apply to the whole repository. The project is a Rust 2024 desktop application and library; preserve the separation between blackjack mechanics, learning logic, persistence, and the desktop renderer.

## Worktree workflow

Implementation and repository edits for this codebase MUST be performed by an omp agent in a separate Orca-managed worktree. The coordinating/original checkout remains read-only; merge back only on explicit user request. An agent already assigned to this isolated worktree implements there without spawning another worktree.

## Rust API design

Use the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) as the default for public and cross-module Rust APIs. Apply the relevant rule rather than copying the upstream checklist mechanically:

- Prefer the standard library and existing project types before adding abstractions or dependencies.
- Make names predictable: `snake_case` functions, `CamelCase` types, no `get_` prefix for ordinary accessors, and consistent terminology for the same concept.
- Expose invariants through types and constructors. Prefer enums, newtypes, and dedicated domain types over ambiguous booleans, stringly typed values, or invalid intermediate states.
- Keep public APIs unsurprising: cheap accessors should be cheap, iterators should implement the expected iterator traits, conversions should use the conventional `From`/`Into`/`TryFrom` traits, and fallible operations should return `Result` rather than panic.
- Use `Default` only when there is a clear sensible default. Use builders only when construction genuinely has many optional parameters; do not add a builder for a small struct.
- Avoid unnecessary cloning, allocation, copying, and public fields. Borrow when ownership is not required, but do not contort an API to avoid a justified allocation.
- Derive or implement the standard traits that make domain types useful (`Debug`, equality, ordering, hashing, serialization) when their semantics are correct. Keep trait behavior consistent with the type's meaning.
- Document public items and explain non-obvious invariants, panic conditions, errors, and serialization compatibility. Examples and doctests should show the supported usage.
- Preserve forward compatibility: avoid needless exhaustive public representations and avoid exposing implementation details that callers may depend on.

## Change discipline

Before changing an exported type, function, enum, serialized representation, or module boundary, inspect its callers and tests. Prefer a small compatible API over a speculative abstraction. Update all affected call sites and documentation in the same change. Run `cargo fmt --check`, `cargo check`, and the relevant tests after Rust changes; use Clippy when available for API or idiom changes.

The upstream guidelines are the authority for details and rationale. Project requirements and an explicitly documented compatibility constraint take precedence over a guideline when they conflict.

## Native UI visual tests

Every visual change MUST include one committed semantic happy-path scenario and one relevant failure-path assertion. Screenshots are supporting evidence, not the primary pass/fail oracle.

- Prefer accessibility-tree assertions over coordinates.
- Use stable text/role locators, not widget IDs or pixel positions.
- Do not assert exact card ranks because the shoe is randomized.
- Do not use screenshot pixel baselines for dynamic cards, fonts, or cross-platform rendering.
- Save screenshots as CI artifacts for review; use semantic assertions for pass/fail.
- Keep save-failure and corrupt-database cases in Rust/storage tests, where failures are deterministic.
- Run the UI scenario with an isolated profile and a unique inspection port.
- Keep the inspection endpoint on loopback; it has no authentication.
