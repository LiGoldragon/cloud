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

## On-demand compute provisioning

- The cloud component's active capability is on-demand compute-node
  provisioning, Hetzner first. The lifecycle is create / observe / destroy: an
  ordinary socket observes the live hosts a provider account holds, and a
  meta-policy (owner-approved) socket prepares, approves, and applies host plans
  that create or destroy nodes.
- Host creation and destruction are owner-approved meta operations.
  `PrepareHostPlan` mints a `Create` host plan; `PrepareHostDestruction`
  (`HostDestruction { provider, host_name }`) mints a `Destroy` host plan that
  reuses the `HostPlanPrepared` reply. Both require explicit approval before the
  apply step routes a `Create` plan to provider creation and a `Destroy` plan to
  destroy-by-name (the host is resolved to its provider identifier at apply
  time, so a destroy plan's create-only fields carry no meaning and are minted
  empty).
- The Hetzner adapter resolves its API token fresh from the `HCLOUD_TOKEN`
  credential handle (gopass `hetzner/api-token`); the token is never echoed and
  crosses meta policy only by handle, never as secret bytes.

## Billing-hour reuse pool (Spirit `6ks1`)

- Cloud providers bill by the hour, so a node should not be torn down and
  re-created within a paid hour it has already been charged for. Each provider
  carries a `keep_warm_duration` (Hetzner: 59 minutes) anchored to the node's
  `created_at`: an idle node still inside its paid window is kept warm and
  reused for the next workload rather than destroyed, and is reaped only just
  before the next paid hour would begin.
- The principle is reuse-before-reap, but the larger win is latency: a warm,
  already-provisioned node answers far faster than a cold create. The reuse pool
  and its reaper ride the deferred-effect / actor seam and are a staged follow-up
  to Phase 1, not yet built.

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
  ordinary Signal records; secret material crosses meta policy only by handle.
- `cloud-daemon` starts from one signal-encoded rkyv
  `DaemonConfiguration` file. Inline NOTA and `.nota` configuration
  files are CLI/authoring surfaces and are rejected by the daemon
  entrypoint.
- `cloud-daemon` uses the emitted actor-native listener spine for ordinary and
  meta sockets; component code owns only the cloud-specific runtime hooks and
  the meta contract frame handling that the generator cannot derive yet.
- `cloud-daemon` sockets carry length-prefixed generated schema frames. The
  prior `ExchangeFrame` handshake transport is retired and should not be
  reintroduced as a second live path.

## Principles

- Schema-driven reports and implementation work should show what the authored
  schema produces: typed interfaces, derived traits, implementation boundaries,
  and where the derivation exposes design flaws.
- Operator integrates designer `next` work into `main` by rebasing,
  cherry-picking, re-implementing, or merging when the code is good enough.

*Source statements live in Spirit records under the `cloud`, `schema`, `signal`,
`component-triad`, and `branches` topics.*
