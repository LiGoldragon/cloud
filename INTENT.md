# INTENT — cloud

*What the psyche has explicitly intended for this project. Synthesised from
Spirit records; not embellished.*

## Goals

- `cloud` upgrades toward the schema-interface and triad-engine approach while
  `main` remains the operator-owned integration path and `next` carries breaking
  contract work.
- The cloud runtime owns provider execution: policy state, plan state,
  credential-handle resolution, provider effects, and future SEMA persistence.

## Constraints

- Signal contract repositories carry only Signal wire vocabulary. Runtime
  planes, provider effects, REST/provider adapters, storage, and broader daemon
  behavior belong in the runtime component or the relevant schema/runtime repo.
- Runtime plane schemas should be implementation artifacts, not sketches. Missing generator support is a blocker to name explicitly, not a
  reason to leave sketch files as the destination.
- Daemon runtime schema generation uses the shared `schema_rust_next::build`
  driver. The cloud build consumes ordinary and meta Signal contract schemas
  through Cargo schema metadata and must not hard-code workspace checkout paths
  to compensate for missing metadata.
- Provider credentials and secret bytes do not belong in source, logs, or
  ordinary Signal records; secret material crosses owner policy only by handle.

## Principles

- Schema-driven reports and implementation work should show what the authored
  schema produces: typed interfaces, derived traits, implementation boundaries,
  and where the derivation exposes design flaws.
- Operator integrates designer `next` work into `main` by rebasing,
  cherry-picking, re-implementing, or merging when the code is good enough.

*Source statements live in Spirit records under the `cloud`, `schema`, `signal`,
`component-triad`, and `branches` topics.*
