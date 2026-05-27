# INTENT — cloud

*What the psyche has explicitly intended for the `cloud` component.
Synthesised from spirit intent records; not embellished.*

## Goals

- `cloud` is the home for **provider API machinery** including
  Cloudflare, Google Cloud, and cloud hosters such as Hetzner
  (record 296, Decision, Medium).
- Per record 281 (Decision, Medium): *"cloud owns cloud-provider
  API management."*
- Per record 294 (Decision, Medium): create cloud as the cloud API
  management triad, with Cloudflare DNS, Cloudflare settings, and
  redirect rules as the first target.
- First production target: **Cloudflare DNS records and similar
  Cloudflare-managed resources** (record 680, Maximum; record 685,
  Medium; record 282, Medium).
- Per record 684 (Decision, Maximum): cloud component production
  push is prioritized; first MVP can skip the new schema-engine
  approach to ship faster.

## Constraints

- Per record 679 (Constraint, Maximum): the production slice uses
  the **old Rust signal-macro path**; schema-engine cutover is
  deferred for this MVP (per record 684).
- Per record 325 (Decision, Maximum): **cloud plan preparation
  belongs on the owner signal surface** — owner-signal-cloud
  carries the privileged operations; signal-cloud carries the
  ordinary observations.
- Per record 342 (Decision, Maximum) + record 295 (Principle,
  Minimum): provider integrations may be **build-time opt-ins**,
  and capability observation must distinguish built-but-
  unconfigured providers from providers not built into the daemon.
- Per record 689 (Constraint, Maximum): API key via
  **env-var-populated-by-password-manager pattern** (the existing
  FEMOS utility pattern); no credentials in source, logs, or
  ordinary Signal records.

## Principles

- Per record 686 (Principle, Medium): the cloud daemon starts
  **almost-stateless**; it caches last-known-state of queried
  Cloudflare resources. Cache loss is acceptable because
  **Cloudflare is the source of truth**.
- Per record 681 (Principle, Maximum): cloud first state may be a
  **lossy provider-backed cache**.
- Per record 687 (Decision, Medium): first cloud cache is
  **runtime / volatile (in-memory)**.
- Per record 683 (Principle, Medium): cloud signal language **starts
  small and typed**.
- Per record 688 (Decision, Medium): **prefer the Cloudflare CLI
  (`flarectl`) over direct HTTP API** for first integration if
  easier.
- Per record 682 (Decision, Medium): Cloudflare credential starts
  as an **environment token**.
- Per record 284 (Principle, Minimum): capability-missing daemons
  may eventually self-upgrade before unsupported replies — future
  direction, not the MVP.
- Per record 283 (Principle, Minimum): provider integrations may
  be build-time opt-ins.

## Cloud is a Nexus-IO component (record 965, Maximum)

Per spirit record 965 (Maximum, 2026-05-27):

> *"NEXUS specifically covers any layer where code runs in
> response to typed input and returns typed output - internal IO,
> **external calls (e.g. cloud component starting Cloudflare CLI
> to change DNS)**, AND all user interfaces."*

The cloud-to-Cloudflare CLI interaction is the **canonical
nexus-IO example** the psyche named in record 965. The cloud
daemon's Cloudflare-facing logic — `flarectl` shell-outs, HTTP
API adapters, plan preparation, plan application — is a **nexus
schema** in the three-schema-type framing. The cloud daemon
exposes:

- **Signal schemas** — `signal-cloud` (ordinary observations,
  reads, plan listing) + `owner-signal-cloud` (privileged plan
  preparation + apply per record 325) — declare the wire surface.
- **Nexus schemas** — the cloud daemon's execution-layer code
  that runs `flarectl` and HTTP API calls in response to typed
  input and returns typed output.
- **Sema schemas** — durable state for accounts, prepared plans,
  rate-limit ledgers (currently deferred to in-memory store per
  record 687).

## Nexus is the MAIL KEEPER — cloud-daemon flow (record 970, Maximum)

Per spirit record 970 (Maximum, 2026-05-27):

> *"NEXUS is the mail keeper - the in-between runtime layer that
> owns mail tracking and Signal-to-SEMA translation; when Nexus
> has the mail, the mail is in the BEING-PROCESSED state; Nexus
> IS the runtime representation that a mail is being processed."*

The cloud daemon has **THREE EXECUTION CENTERS**: Signal
(communication), Nexus (execution + mail keeper + translator —
where `flarectl` calls happen), SEMA (state — accounts, plans,
caches). The complete flow for a cloud message — e.g. an
owner-approved DNS plan apply:

```text
Signal IN
  -> Nexus accepts mail (mail enters BEING-PROCESSED state)
     [on_sent hook fires here — record 963]
  -> Nexus translates input + runs flarectl / HTTP API
     (the EXTERNAL CALL — record 965's canonical nexus-IO example)
  -> Nexus translates response (Cloudflare reply + state change)
  -> Nexus emits SEMA query (cache update + plan status)
  -> SEMA reply (with database marker — record 935)
  -> Nexus translates SEMA reply to Signal response
Signal OUT
```

Record 970 **CONSOLIDATES** records 935 + 963 + 964 + 965 into
one unified picture. For cloud, this confirms the architecture:
external provider calls are **specific instances of the more
fundamental in-between translator + mail keeper role**. The cloud
daemon is a normal three-execution-center daemon; the Cloudflare
CLI shell-out is a Nexus-side external IO, not a separate
architectural layer.

## Anti-patterns

- Per record 295 (Principle, Minimum) + ARCHITECTURE.md §"Hard
  Constraints": no direct provider calls from the CLI; the CLI is
  thin runtime machinery; all provider IO lives in the daemon's
  Nexus plane.
- ARCHITECTURE.md §"Hard Constraints": no provider credentials in
  source, logs, or ordinary Signal records; secret material crosses
  owner policy only by handle.
- ARCHITECTURE.md §"Hard Constraints": no deprecated `signal-core`
  dependency in new code.
- ARCHITECTURE.md §"Hard Constraints": Cloudflare is a provider
  adapter, not the domain model — `domain-criome` owns domain
  meaning; `cloud` does not decide which Criome domains should
  exist; it applies provider-facing plans from authorized inputs.

## Recurring patterns realised in this repo

Per spirit record 988 (Maximum, 2026-05-27) + workspace INTENT.md
§"Recurring architectural patterns": cloud is a normal
three-execution-center daemon with the Cloudflare CLI shell-out
as a Nexus-side external IO. cloud realises:

- **Pattern A — Async lives at the data-type level.** Cloud's
  signal messages carry their own lifecycle state; provider-call
  results return as typed replies through the same mail mechanism
  every other daemon uses (records 935, 962, 963, 970).
- **Pattern B — Three execution centers.** Cloud realises the
  general daemon shape; the Cloudflare CLI is specifically a
  Nexus-side external IO (record 965 names this exact example).
- **Pattern C — Methods on schema-generated data types.** Cloud's
  hand-written Rust attaches behaviour to schema-emitted nouns;
  no free helpers, no ZST namespace holders.

## Continuous manifestation

Per spirit record 944 (Maximum, 2026-05-27): this `INTENT.md` is
maintained continuously as new psyche intent affecting cloud
lands. See `~/primary/skills/repo-intent.md` §"Continuous
manifestation discipline".

---

*Source statements live in Spirit intent records under topic*
*`cloud` (and runtime-architecture topics that bind cloud as a*
*Nexus-IO component). Read the deployed Spirit via the standard*
*query path; the records named above are the canonical anchors.*
