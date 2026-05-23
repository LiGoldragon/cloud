# cloud

Criome cloud-provider API runtime.

This repo ships the `cloud-daemon` runtime and its bundled thin `cloud` CLI.
The CLI is a text-to-Signal adapter and has exactly one peer: `cloud-daemon`.

The current runtime slice has ordinary and owner Unix sockets, `signal-frame`
request/reply handling, in-memory account policy and plan state, and a
Cloudflare read-only provider path for DNS zones and records. The Cloudflare
credential handle names an environment variable, commonly
`CLOUDFLARE_DNS_TOKEN`, populated by the caller's secret manager before the
daemon starts.

Live provider mutation is intentionally not faked; approved apply requests
currently return a typed owner rejection until the mutation actor exists.
