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
- prepare provider-specific plans;
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

## First Implementation Slice

1. Bind ordinary and owner Unix sockets.
2. Decode `signal-cloud` and `owner-signal-cloud` frames.
3. Return typed unsupported/configuration replies when no provider account is
   configured.
4. Store provider policy and plan records in sema-engine.
5. Add a Cloudflare read-only actor for zones and records.
6. Add Cloudflare plan generation.
7. Add owner-approved Cloudflare apply.

## Hard Constraints

- No provider credentials in source, logs, or ordinary Signal records.
- Secret material crosses owner policy only by handle.
- No direct provider calls from the CLI.
- No deprecated `signal-core` dependency in new code.
- Cloudflare is a provider adapter, not the domain model.
