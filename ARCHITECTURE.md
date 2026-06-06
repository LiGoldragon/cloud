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

`sema-engine` persistence is intentionally deferred because the current engine
still pulls the deprecated `signal-core` dependency. The store boundary is kept
small so persistence can be swapped in after that dependency is removed.

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
daemon runtime schemas: `schema/nexus.schema` targets `NexusRuntime`, and
`schema/sema.schema` targets `SemaRuntime`. The build consumes the ordinary
`signal-cloud` schema directory and the meta `meta-signal-cloud` schema
directory from Cargo metadata, validates each authored schema as a
`SchemaSource` object through text and rkyv round-trips, then
freshness-checks `src/schema/{nexus,sema}.rs`. The daemon must not hard-code
local checkout paths for contract schemas.

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

`src/schema_daemon.rs` wires that engine to a live daemon. `SchemaDaemon`
binds the ordinary + owner sockets on one `triad_runtime::MultiListenerDaemon`,
each tagged by a `ListenerRole` (`Ordinary` / `Owner`). `handle_stream` reads
the length-prefixed wire body, decodes the arriving role's contract `Input`
through the schema-emitted `decode_signal_frame`, wraps it in a
`nexus::SignalInput`, drives it through `NexusEngine::execute` (the generated
`Runner` continuation loop), and writes the contract `Output` back as a
length-prefixed `encode_signal_frame` body. `src/schema_role.rs` carries the
empty `triad-runtime` role-marker impls (`NexusWork`, `SemaWriteInput`, …) on
the schema-emitted nouns that the 0.2.1 `RunnerEngines` bound requires until the
cloud schema artifacts are regenerated against the newer emitter (which emits
them inline).

The newest core crates now emit a component daemon spine around the same
triad-runtime primitives. The current direction is to evolve that emitted spine
so it hosts the load-bearing daemon properties already proven by the new lojix
stack, then let `cloud` adopt the generated/common daemon rather than keep a
permanent fork. Any generated/common daemon adoption must preserve the
first-class ordinary + meta contract routing into `nexus::SignalInput`, the
per-request `SchemaRuntime` over shared `SchemaStore`, bounded length-prefixed
frame handling with read timeouts, non-blocking provider work, and owner-socket
peer-credential-derived origin/authority handling. Owner credential mismatch is
a fail-closed rejection. Until the emitted spine hosts those properties and
provider IO lives on the schema effect plane, `SchemaDaemon` remains the
cloud-local schema-engine witness and the production daemon remains separate.

The schema-engine daemon does not yet perform live Cloudflare IO (its
`run_effect` reports empty provider listings) or engine-side diff-aware plan
generation; those still live on the legacy [`crate::daemon::Daemon`] over
`signal_frame::ExchangeFrame` + the hand-written `Store`, which `cloud-daemon`
runs as the production runtime. `SchemaDaemon` is the build-verified,
socket-tested schema-engine path; the `cloud-daemon` cutover lands once the
effect plane carries the Cloudflare IO. Durable `sema-engine` / redb backing
remains the noted follow-on (deferred while `sema-engine` still pulls the
deprecated `signal-core` dependency).
