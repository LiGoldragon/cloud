# cloud

Criome cloud-provider API runtime.

This repo ships the `cloud-daemon` runtime and its bundled thin `cloud` CLI.
The CLI is a text-to-Signal adapter and has exactly one peer: `cloud-daemon`.

The current runtime slice has ordinary and meta Unix sockets, `signal-frame`
request/reply handling, in-memory account policy and plan state, validation,
diff-aware DNS plan preparation, and a Cloudflare provider path for DNS zones
and records. The production default uses `flarectl --json` under the
daemon-owned provider client for Cloudflare DNS read and meta-approved DNS
record application. The packaged `flarectl` wrapper loads `CF_API_TOKEN` from
`gopass show -o cloudflare/api-token` and fails loudly if that handle is
missing.

`cloud` can now also prepare provider plans from `domain-criome` projections:
`domain-criome` owns provider-neutral records/redirects, while `cloud` chooses
the configured provider and lowers the projection into the existing meta
`PreparePlan` / `ApprovePlan` / `ApplyPlan` ceremony.

Redirect mutation is intentionally not faked; Cloudflare Page Rules are legacy
read-only material and modern Rulesets support remains a future slice.
