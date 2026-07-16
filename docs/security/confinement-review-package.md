# Confinement Review Package (Human-Gated)

**Status:** DRAFT for security review — **not** production-enforced  
**Cites:** ADR-008, ADR-016, SDS §12.2, IG AI-6 / AGENTS §7  
**Rule:** An agent may draft; a human owner must review before **enforcement** lands.  
**Never** weaken a filter, downgrade “indeterminate,” or claim full sandboxing while mode is Advisory.

---

## 1. Current mode

| Item | Value |
|---|---|
| `ConfinementMode` | **Advisory** |
| Child lockdown | Logs intended profile; does not fail closed |
| Parent confine (Windows) | Logs intended AppContainer + job; not applied |
| Public claim | “Advisory confinement hooks present” — **not** “sandboxed” |

Enforcement (`ConfinementMode::Enforced`) requires:

1. Signed review of this package  
2. Superseding ADR or amendment note if policy changes  
3. CI test that proves deny paths without breaking legitimate worker work  
4. Explicit enablement flag / build feature (default off until review)

---

## 2. Threat model (Z1 worker)

| Asset | Threat | Confinement intent |
|---|---|---|
| Host filesystem | Malicious PDF escape writes host files | Deny open of arbitrary paths; only inherited doc/shmem handles |
| Network | Exfil / C2 | Deny socket/connect |
| Process | Spawn shell / inject | Deny execve / CreateProcess |
| Other processes | Read peer memory | Deny ptrace / process_vm_* |
| Privilege | Escape to Z0 | Process isolation + no broker from script |

Broker remains in Z0 for any future privileged op (ADR-016).

---

## 3. Target profiles (to implement after review)

### Linux
- Namespaces: user, mount (private), network (none)  
- seccomp-bpf: allowlist only (see `sandbox::confinement::LINUX_SYSCALL_ALLOWLIST` in code)  
- Fail-closed for unknown syscalls after review  

### Windows
- AppContainer with minimal capabilities  
- Job object: kill-on-close, memory + CPU caps  
- Named pipe ACL for worker SID only  

### macOS
- `sandbox_init` profile: deny network + FS except inherited FDs  

---

## 4. Review checklist (human)

- [ ] Allowlist reviewed for PDFium/Skia needs (mmap, futex, threads)  
- [ ] No network path remains  
- [ ] Inherited handles still work (doc + shmem + IPC)  
- [ ] Kill-worker respawn still passes  
- [ ] Advisory → Enforced is an explicit opt-in feature flag  
- [ ] Docs/README do not claim “sandboxed” until Enforced  

**Reviewer:** _________________ **Date:** ________ **Decision:** Advisory / Enforced (feature-gated)

---

## 5. Code entry points

| Function | Role |
|---|---|
| `lockdown_worker()` | Child-side entry (worker-main first call) |
| `confine_child()` | Parent-side (Windows) |
| `confinement_report()` | Machine-readable status for diagnostics |

Do **not** remove advisory logging until Enforced ships with tests.
