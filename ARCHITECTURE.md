# cloud Architecture

`cloud` is the provider API daemon for Criome systems. Its first target is
Cloudflare DNS records and redirect rules. Later provider actors can cover
Google Cloud DNS, Hetzner Cloud, and other cloud APIs.

## Triad

- Runtime repo: `cloud`.
- Ordinary contract: `signal-cloud`.
- Meta contract: `meta-signal-cloud`.

The CLI is bundled runtime machinery, not a separate triad leg. The CLI has
exactly one Signal peer: `cloud-daemon`.

## Boundary

`cloud` owns provider execution:

- observe provider accounts, zones, records, redirects, and capabilities;
- validate desired provider-neutral state;
- prepare provider-specific plans through owner authority;
- apply owner-approved plans;
- track provider rate limits, remote operation identifiers, and failures.

`domain-criome` owns domain meaning and provider-neutral projection. `cloud`
does not decide which Criome domains should exist; it applies provider-facing
plans from authorized inputs.

## Actor Shape

The first daemon should use one actor per concern:

- `CloudflareProvider` for Cloudflare HTTP API calls;
- `PlanStore` for prepared plans and approval state;
- `PolicyStore` for account, credential-handle, capability, and zone policy;
- `RateLimitGate` for provider rate-limit and retry state;
- `RemoteOperationTracker` for asynchronous provider operations.

Provider calls must not block the ordinary listener, owner listener, or plan
store. Slow provider work belongs behind provider actors with timeouts.

## Current Implementation Slice

1. Bind ordinary and owner Unix sockets.
2. Decode `signal-cloud` and `meta-signal-cloud` frames.
3. Return typed unsupported/configuration replies when no provider account is
   configured.
4. Store account policy, prepared plans, and lossy last-known provider reads
   through a runtime store abstraction.
5. Generate local plans from `meta-signal-cloud::PlanPreparation`.
6. Require owner approval before apply.
7. Resolve Cloudflare credential handles through environment variables and list
   Cloudflare zones and DNS records through the daemon-owned provider client.
8. Apply owner-approved DNS-record plans through the daemon-owned Cloudflare
   provider client. The production default uses `flarectl --json` for DNS
   record get/set; `HttpApi` remains available as a direct-API adapter.
9. Accept `domain-criome` provider-neutral projections through the owner
   contract, lower them into provider plans, and apply those plans through the
   same approval ceremony.
10. Validate DNS desired-state content enough to reject malformed IPv4/IPv6
   records and malformed redirect targets before planning.
11. Prepare diff-aware DNS plans by comparing desired records against the last
   provider state: create new records, update changed records, and delete
   records omitted from desired state.

`sema-engine` persistence is still a follow-on implementation slice, not a
current dependency blocker: `sema-engine` is now clean of the retired
`signal-core` crate. The store boundary is kept small so the current
`SchemaStore` tables can move behind `sema-engine` without widening the daemon
surface.

Cloudflare DNS observation and DNS-record application are production-shaped:
ordinary reads use the ordinary Signal socket, owner-approved application uses
the meta Signal socket, both read only owner-registered accounts and zones,
and the daemon caches the last known record listing after Cloudflare accepts a
read or mutation. Redirect observation and redirect mutation are future slices;
until the Rulesets/Page-Rules read path exists, redirect requests return typed
unsupported replies rather than silent empty listings.

## Hard Constraints

- No provider credentials in source, logs, or ordinary Signal records.
- Secret material crosses owner policy only by handle.
- No direct provider calls from the CLI.
- No deprecated `signal-core` dependency in new code.
- Cloudflare is a provider adapter, not the domain model.
- `cloud-daemon` starts from one signal-encoded rkyv
  `DaemonConfiguration` file. Inline NOTA and `.nota` files are
  rejected by the daemon entrypoint; NOTA remains at the CLI/authoring
  edge.

## Schema-engine upgrade track

`main` keeps the production-shaped runtime on the hand-written Rust +
`signal_channel!` path while also carrying source-visible schema artifacts for
the daemon runtime planes. The schema placement is split by runtime plane:

- `signal-cloud` — ordinary working Signal schema only, published from
  `schema/lib.schema` through Cargo schema metadata.
- `meta-signal-cloud` — meta (owner-only policy) policy Signal schema only,
  published from `schema/lib.schema` through Cargo schema metadata.
- `cloud/schema/nexus.schema` — daemon-owned Nexus decision/effect
  plane schema; imports contract `Input`/`Output` roots and SEMA roots.
- `cloud/schema/sema.schema` — daemon-owned SEMA state plane schema;
  owns state-transition and table identity language.

Signal contract repositories carry only the wire vocabulary that clients send
and receive. Nexus decisions, SEMA state, provider effects, REST/provider
adapters, policy state, plan state, credential-handle resolution, and future
sema-engine persistence belong in the `cloud` runtime crate.

`cloud/build.rs` is wired to the shared `schema_rust_next::build` driver for
daemon runtime schemas: `schema/nexus.schema` targets `NexusRuntime`,
`schema/sema.schema` targets `SemaRuntime`, and a `daemon_module` emission
driven by a two-tier `NexusDaemonShape` targets the emitted daemon (triad_main).
The daemon shape declares a `cloud-daemon` process whose working tier is a
**dependency-crate contract** (`WorkingListenerTier::dependency("signal_cloud::schema::lib")`
— cloud's triad keeps the ordinary contract in `signal-cloud`, not a locally
emitted module) plus an owner-only meta tier (`meta-signal-cloud`, mode `0o600`).
The build consumes the ordinary `signal-cloud` and meta `meta-signal-cloud`
schema directories from Cargo metadata, validates each authored schema as a
`SchemaSource` through text and rkyv round-trips, then freshness-checks
`src/schema/{nexus,sema,daemon}.rs`. The daemon must not hard-code local
checkout paths for contract schemas. (cloud's `nexus.schema` / `sema.schema`
enums use the **pair declaration form** `[(Variant Type) …]` for
payload-carrying variants — the emitter reads a bare-name body-enum entry as a
unit variant.)

`src/schema_runtime.rs` is the implementation slice over those generated planes.
It implements the generated Nexus and SEMA engine traits over a durable
[`SchemaStore`] (`src/schema_store.rs`): it triages ordinary
capability/validation requests, triages meta registration/policy/plan requests,
applies SEMA writes, observes SEMA reads, and turns SEMA completions back into
Signal replies. The two schema-emitted SEMA tables back the state —
`AccountPolicyTable` keyed by provider + account, and `PlanTable` as the 1:N
keyed collection of `StoredPlan` keyed by plan identifier (report 77's interim
in-memory workaround, requiring no `sema-engine` identified-multi-key primitive).
Each request is served by its own `SchemaRuntime` over a clone of the shared
`Arc<SchemaStore>`, so concurrent requests share the durable tables.
`SchemaRuntime::reply_to_signal` is the per-request execute helper — it builds
one engine over the shared store, drives the Nexus continuation to its terminal
`ReplyToSignal`, and returns the `SignalOutput` — and lives on the engine noun
so the emitted daemon hooks call it rather than carrying logic on a marker type.

The daemon spine is now **emitted** into `src/schema/daemon.rs` by the
schema-rust-next daemon emitter (triad_main): the `DaemonCommand` argv→config
parse, the working decode→execute→encode `GeneratedDaemonRuntime`, the two-tier
`MultiListenerDaemon` bind (working + owner-only meta, `ListenerTier::Working` /
`ListenerTier::Meta`), `DaemonError`, and the `DaemonEntry` exit. `src/schema_daemon.rs`
now hand-writes only the record-1488 escape hatches — `impl ComponentDaemon for
CloudDaemon`: `build_runtime` (the shared `Arc<SchemaStore>`), `handle_working_input`
(one ordinary `Input` → `Output` via `reply_to_signal`), and the owner-only
`handle_meta_stream` (component-owned meta wire codec, decoding
`meta_signal_cloud` frames) — plus a thin `SchemaDaemon::new(config).run()`
wrapper over the emitted binder for tests/in-process launchers. (The prior
hand-written `SchemaDaemon`/`CloudRuntime`/`serve_*`/`ListenerRole` plumbing and
the `src/schema_role.rs` role-marker bridge are retired — the role-marker impls
are emitted inline now.)

The schema-engine daemon does not yet perform live Cloudflare IO (its
`run_effect` reports empty provider listings) or engine-side diff-aware plan
generation; those still live on the legacy [`crate::daemon::Daemon`] over
`signal_frame::ExchangeFrame` + the hand-written `Store`, which `cloud-daemon`
runs as the production runtime. The emitted-triad_main schema-engine path
(`CloudDaemon` via `src/schema/daemon.rs`) is build-verified and socket-tested
(the live `schema_daemon` tests drive both tiers over real Unix sockets); the
`cloud-daemon` cutover lands once the effect plane carries the Cloudflare IO.
Durable `sema-engine` backing remains the noted follow-on: the dependency
blocker is gone, and the remaining work is replacing the current `SchemaStore`
table implementation with sema-engine-owned database operations.
