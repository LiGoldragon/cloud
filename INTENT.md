# INTENT — cloud

*What the psyche has explicitly intended for this project. Synthesised from
Spirit records; not embellished.*

## Goals

- `cloud` upgrades toward the schema-interface and triad-engine approach while
  `main` remains the operator-owned integration path and `next` carries breaking
  contract work.
- The current implementation should be written fresh from the desired cloud
  component shape, not treated as constrained by the earlier prototype.
- The cloud runtime owns provider execution: policy state, plan state,
  credential-handle resolution, provider effects, and future SEMA persistence.
- The latest core-crate daemon-emission work is an alignment target for `cloud`.
  The current direction is to evolve the shared emitter/runtime so it hosts the
  load-bearing daemon properties, then let `cloud` adopt generated/common daemon
  machinery without regressing cloud's authority, concurrency, provider-effect,
  or store boundaries.

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
- The schema-engine cutover must not regress the two typed authority-tiered
  contracts into one daemon-local Nexus/SEMA runtime, per-request runtime over
  shared store state, bounded frame/read handling, non-blocking provider work,
  or owner-socket authority hardening.
- Core-crate generated daemon surfaces may replace local daemon scaffolding only
  after they can host those cloud requirements; until then, hand-written cloud
  daemon code remains a valid reference for missing generic runtime behavior.
- Owner/meta socket peer-credential checks should fail closed on an unauthorized
  uid rather than tagging the request and letting privileged verbs fail later.

## Principles

- Schema-driven reports and implementation work should show what the authored
  schema produces: typed interfaces, derived traits, implementation boundaries,
  and where the derivation exposes design flaws.
- Operator integrates designer `next` work into `main` by rebasing,
  cherry-picking, re-implementing, or merging when the code is good enough.
- When cloud has to hand-write logic that is generic across components, that
  friction feeds back into `schema-rust-next` and `triad-runtime` instead of
  becoming permanent cloud-only boilerplate.

*Source statements live in Spirit records under the `cloud`, `schema`, `signal`,
`component-triad`, and `branches` topics.*
