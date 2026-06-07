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
- prepare provider-specific plans through meta authority;
- apply meta-approved plans;
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

Provider calls must not block the ordinary listener, meta listener, or plan
store. Slow provider work belongs behind provider actors with timeouts.

## Current Implementation Slice

1. Bind ordinary and meta Unix sockets.
2. Decode `signal-cloud` and `meta-signal-cloud` frames.
3. Return typed unsupported/configuration replies when no provider account is
   configured.
4. Store account policy, prepared plans, and lossy last-known provider reads
   through a runtime store abstraction.
5. Generate local plans from `meta-signal-cloud::PlanPreparation`.
6. Require meta approval before apply.
7. Resolve Cloudflare credential handles through environment variables and list
   Cloudflare zones and DNS records through the daemon-owned provider client.
8. Apply meta-approved DNS-record plans through the daemon-owned Cloudflare
   provider client. The production default uses `flarectl --json` for DNS
   record get/set; `HttpApi` remains available as a direct-API adapter.
9. Accept `domain-criome` provider-neutral projections through the meta
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
ordinary reads use the ordinary Signal socket, meta-approved application uses
the meta Signal socket, both read only meta-registered accounts and zones,
and the daemon caches the last known record listing after Cloudflare accepts a
read or mutation. Redirect observation and redirect mutation are future slices;
until the Rulesets/Page-Rules read path exists, redirect requests return typed
unsupported replies rather than silent empty listings.

## Hard Constraints

- No provider credentials in source, logs, or ordinary Signal records.
- Secret material crosses meta policy only by handle.
- No direct provider calls from the CLI.
- No deprecated `signal-core` dependency in new code.
- Cloudflare is a provider adapter, not the domain model.
- `cloud-daemon` starts from one signal-encoded rkyv
  `DaemonConfiguration` file. Inline NOTA and `.nota` files are
  rejected by the daemon entrypoint; NOTA remains at the CLI/authoring
  edge.

## Schema-engine upgrade track

`main` now runs the production-shaped runtime through the emitted actor-native
daemon spine while still preserving the existing provider `Store` behavior
behind a schema bridge. The schema placement is split by runtime plane:

- `signal-cloud` — ordinary working Signal schema only, published from
  `schema/lib.schema` through Cargo schema metadata.
- `meta-signal-cloud` — meta policy Signal schema only,
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
emitted module) plus a meta tier (`meta-signal-cloud`, mode `0o600`).
The build consumes the ordinary `signal-cloud` and meta `meta-signal-cloud`
schema directories from Cargo metadata, validates each authored schema as a
`SchemaSource` through text and rkyv round-trips, then freshness-checks
`src/schema/{nexus,sema,daemon}.rs`. The daemon must not hard-code local
checkout paths for contract schemas. (cloud's `nexus.schema` / `sema.schema`
enums use the **pair declaration form** `[(Variant Type) …]` for
payload-carrying variants — the emitter reads a bare-name body-enum entry as a
unit variant.)

`src/schema_runtime.rs` remains the pure schema-engine implementation slice over
those generated planes. It implements the generated Nexus and SEMA engine traits
over [`SchemaStore`] (`src/schema_store.rs`): it triages ordinary
capability/validation requests, triages meta registration/policy/plan requests,
applies SEMA writes, observes SEMA reads, and turns SEMA completions back into
Signal replies. The two schema-emitted SEMA tables back the state —
`AccountPolicyTable` keyed by provider + account, and `PlanTable` as the 1:N
keyed collection of `StoredPlan` keyed by plan identifier. That slice is still
valuable as the schema/Nexus/SEMA pilot, but it is not the live provider-effect
path yet.

The live daemon spine is **emitted** into `src/schema/daemon.rs` by the
schema-rust-next daemon emitter (triad_main): the `DaemonCommand` argv-to-config
parse, the async working decode/handle/encode runtime, the two-tier
`ActorMultiListenerDaemon` bind (working + meta, `ListenerTier::Working` /
`ListenerTier::Meta`), `DaemonError`, and the `DaemonEntry` exit.
`src/schema_daemon.rs` hand-writes only the component hooks:
`impl ComponentDaemon for CloudDaemon`, whose runtime is `Arc<Store>`.
Ordinary frames are generated `signal_cloud::schema::lib::Input` values and meta
frames are generated `meta_signal_cloud::schema::lib::Input` values; the
component converts them through `src/schema_bridge.rs` and delegates to the
existing provider `Store`.

This retires the prior hand-written blocking daemon and the old
`ExchangeFrame`/handshake transport. `src/daemon.rs` and `src/frame_io.rs` no
longer exist. `cloud-daemon` now uses length-prefixed schema frames over both
sockets, while the CLI remains a NOTA edge adapter that parses the existing
ordinary/meta operations and sends the generated schema frame to the daemon.
Durable `sema-engine` backing and moving Cloudflare IO fully into the schema
effect plane remain follow-on slices; the current cutover deliberately preserves
provider behavior first, then lets the pure engine catch up without keeping two
live socket stacks.
