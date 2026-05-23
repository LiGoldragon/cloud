# First Cloudflare Slice

The first provider slice should be read-first:

1. Load owner policy with a Cloudflare account and credential handle.
2. Resolve the handle through the eventual secret provider.
3. List zones.
4. List domain-name-system records for an allowed zone.
5. List redirect rules through the Rulesets API.
6. Generate a plan from `signal-cloud::DesiredState`.
7. Require `owner-signal-cloud::ApplyPlan` for mutation.

Do not implement Page Rules as a first-class mutation surface. They are legacy
import/read-only material unless the owner explicitly asks for a migration or
deletion operation.

The runtime already provides the daemon sockets, thin CLI, policy loading, plan
generation, approval gate, and typed apply rejection. The next Cloudflare slice
must replace the current policy-derived/empty observations with a real
read-only provider actor before any mutation is added.
