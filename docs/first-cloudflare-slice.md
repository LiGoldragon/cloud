# First Cloudflare Slice

The first provider slice should be read-first:

1. Load owner policy with a Cloudflare account and credential handle.
2. Resolve the handle through an environment variable populated by the caller's
   secret manager.
3. List zones.
4. List domain-name-system records for an allowed zone.
5. List redirect rules through the Rulesets API.
6. Generate a plan through `owner-signal-cloud::PreparePlan`.
7. Require `owner-signal-cloud::ApplyPlan` for mutation.

Do not implement Page Rules as a first-class mutation surface. They are legacy
import/read-only material unless the owner explicitly asks for a migration or
deletion operation.

The runtime provides the daemon sockets, thin CLI, policy loading, plan
generation, approval gate, real Cloudflare DNS observation, and owner-approved
DNS record application. `Observe(Records(...))` resolves the configured zone
through Cloudflare, lists DNS records, and caches the last known record listing
in the runtime store. `ApplyPlan` can create, update, or delete DNS records
after the owner has prepared and approved the plan. Redirect observation and
redirect mutation remain future slices.

The production default reaches Cloudflare through `flarectl --json` for DNS.
The daemon package wraps `cloud-daemon` with `flarectl` in `PATH`; the CLI still
speaks only to `cloud-daemon` and never invokes provider tools directly.
