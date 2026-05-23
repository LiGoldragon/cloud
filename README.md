# cloud

Criome cloud-provider API runtime.

This repo ships the `cloud-daemon` runtime and its bundled thin `cloud` CLI.
The CLI is a text-to-Signal adapter and has exactly one peer: `cloud-daemon`.

The first runtime slice has ordinary and owner Unix sockets, `signal-frame`
request/reply handling, in-memory account policy and plan state, and typed
unsupported/rejected replies when provider authority is absent. Live provider
mutation is intentionally not faked; approved apply requests currently return a
typed owner rejection until the provider actor exists.
