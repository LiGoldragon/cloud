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
the owner Signal socket, both read only owner-registered accounts and zones,
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
