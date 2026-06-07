# cloud — Agent Instructions

Read `~/primary/AGENTS.md`, then this file.

This repository is the runtime leg of the `cloud` triad:

- `cloud-daemon` will own cloud-provider state, policy, provider actors, and
  sema-engine storage.
- `cloud` will be the thin CLI client that speaks only to `cloud-daemon`.
- `signal-cloud` is the ordinary peer contract.
- `meta-signal-cloud` is the meta policy authority contract.

Do not add a fake CLI that opens files or talks directly to provider APIs.
Until the daemon request path exists, leave binaries unshipped rather than
creating a triad violation.
