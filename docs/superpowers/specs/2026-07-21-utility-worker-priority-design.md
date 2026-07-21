# Utility Worker OS Priority Design

**Requirements:** ADR-008, ADR-009, SDS §2.2.3, NFR-RESP

Document workers retain normal process priority. Utility workers use a distinct sandbox spawn API
that requests below-normal scheduling: `BELOW_NORMAL_PRIORITY_CLASS` on Windows and POSIX `nice
-n 10` on Linux/macOS. Failure to apply the policy is a spawn failure; the system does not silently
run background jobs at interactive priority.

No confinement rule, inherited-handle rule, or broker boundary changes. The utility pool switches
to this explicit API and its existing real-process smoke test exercises successful startup.
