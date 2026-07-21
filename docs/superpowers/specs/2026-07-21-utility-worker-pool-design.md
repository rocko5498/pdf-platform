# Utility Worker Pool Design

**Requirements:** ADR-008, ADR-009, ADR-031, SDS §2.2.3, SDS §4.6

Add a bounded binary command/result codec dedicated to declarative utility jobs. The existing
worker binary recognizes the utility envelope before the document-command envelope. The first
operation registry contains only `noop`; unknown operations fail honestly and remain available
for later operation-specific adapters.

The jobs crate owns a fixed-size pool of sandbox-spawned worker processes. Scheduler threads pick
a slot round-robin, serialize access to that child, send one request, and wait for its correlated
result. Transport loss or an invalid response makes that child unusable: the pool kills/reaps and
replaces it, then returns the typed `WorkerCrashed` outcome so the scheduler's retry-once policy is
the single retry authority. Operation failures do not respawn or retry implicitly.

This slice does not grant file access or implement operation-specific broker calls. Those require
separate capability-reviewed adapters.
