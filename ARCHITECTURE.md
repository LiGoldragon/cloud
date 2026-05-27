# cloud Architecture

`cloud` is the provider API daemon for Criome systems. Its first target is
Cloudflare DNS records and redirect rules. Later provider actors can cover
Google Cloud DNS, Hetzner Cloud, and other cloud APIs.

## Triad

- Runtime repo: `cloud`.
- Ordinary contract: `signal-cloud`.
- Owner contract: `owner-signal-cloud`.

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

## Cloud is a Nexus-IO component (records 965 + 970)

Per spirit records 965 + 970 (Maximum, 2026-05-27): the cloud
component is the **canonical nexus-IO example** the psyche named.
`cloud`'s Cloudflare-facing logic — `flarectl` shell-outs, HTTP
API adapters — is a Nexus schema in the workspace three-schema-type
framing (record 964). The cloud daemon has **THREE EXECUTION
CENTERS**:

| Center | Schema type | What runs in cloud |
|---|---|---|
| **Signal** | `signal-cloud` + `owner-signal-cloud` | Ordinary observation surface + privileged plan-preparation surface |
| **Nexus** | (future) `cloud.nexus.schema` | `flarectl` shell-outs, HTTP API calls, plan execution, rate-limit gating — Nexus is the **MAIL KEEPER**: while Nexus holds the in-flight provider call, the mail is in BEING-PROCESSED state |
| **SEMA** | (future) `cloud.sema.schema` | Account / credential-handle / capability / zone policy + prepared plans + last-known cache (currently in-memory per record 687) |

Per record 970: Nexus is the daemon's mail keeper + Signal-to-SEMA
translator. The on_sent hook (record 963) fires when Signal hands
mail TO Nexus; the database marker (record 935) travels on the SEMA
reply Nexus receives and Nexus propagates it back in the Signal
response.

The schema-engine cutover is deferred per record 684 (Maximum); the
production slice continues on the old Rust signal-macro path per
record 679 (Maximum). The three-execution-center framing applies
architecturally regardless of whether the schema engine drives the
code today; the engine will absorb the existing shape when the
cutover lands.

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
2. Decode `signal-cloud` and `owner-signal-cloud` frames.
3. Return typed unsupported/configuration replies when no provider account is
   configured.
4. Store account policy, prepared plans, and lossy last-known provider reads
   through a runtime store abstraction.
5. Generate local plans from `owner-signal-cloud::PlanPreparation`.
6. Require owner approval before apply.
7. Resolve Cloudflare credential handles through environment variables and list
   Cloudflare zones and DNS records through the daemon-owned provider client.
8. Apply owner-approved DNS-record plans through the daemon-owned Cloudflare
   provider client. The production default uses `flarectl --json` for DNS
   record get/set; `HttpApi` remains available as a direct-API adapter.

`sema-engine` persistence is intentionally deferred because the current engine
still pulls the deprecated `signal-core` dependency. The store boundary is kept
small so persistence can be swapped in after that dependency is removed.

Cloudflare DNS observation and DNS-record application are production-shaped:
ordinary reads use the ordinary Signal socket, owner-approved application uses
the owner Signal socket, both read only owner-registered accounts and zones,
and the daemon caches the last known record listing after Cloudflare accepts a
read or mutation. Redirect observation and redirect mutation are future slices.

## Hard Constraints

- No provider credentials in source, logs, or ordinary Signal records.
- Secret material crosses owner policy only by handle.
- No direct provider calls from the CLI.
- No deprecated `signal-core` dependency in new code.
- Cloudflare is a provider adapter, not the domain model.

## Pending schema-engine upgrade

**Status:** deferred for this production slice. Current cloud work stays on the
hand-written Rust + `signal_channel!` contract path until the schema engine is
ready to absorb the component without delaying Cloudflare DNS management.

**Target:** this component's hand-written `signal_channel!` invocation + Layer 2 Command/Effect + storage types convert to a single `cloud/cloud.schema` file. The brilliant macro library (`primary-ezqx.1`) reads the schema + emits all the wire types + ShortHeader projection + dispatcher + VersionProjection + storage descriptors.

**Sequence:** per `primary-kbmi.1`. Spirit is the MVP pilot landing first via `primary-ezqx.1`; cloud's schema cutover coordinates with cloud daemon implementation. The daemon currently sits at the design-and-skeleton stage (binds sockets, decodes frames, returns unsupported replies); schema cutover lands together with the first real provider-policy storage implementation rather than retrofitting later.

**Per-component concerns:** Per `primary-kbmi.1`; schema cutover coordinates with cloud daemon implementation. The owner-signal-cloud contract is paired with the ordinary signal-cloud contract; both legs of the policy-vs-working split appear in the single `cloud.schema` file (owner-only operations vs ordinary operations) per the schema-language's separation discipline.

**References:**
- `reports/designer/326-v13-spirit-complete-schema-vision.md` — uniform header form + schema-language design
- `reports/designer/324-migration-mvp-spirit-handover-re-specification.md` — migration MVP + handover state
- `reports/designer/322-spirit-mvp-positional-schema-worked-example.md` — Spirit MVP worked example
- `reports/operator/174-schema-import-header-design-critique-2026-05-24.md` — header/body/feature separation + lowering rules
