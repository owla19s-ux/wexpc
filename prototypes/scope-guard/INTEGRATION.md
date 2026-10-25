# Scope reservation / TaskGate prototype integration

Status: PROTOTYPE; tests NOT_RUN. No production dispatcher integration yet.

This standalone crate deliberately does not modify Work-owned Core/desktop files or Fish-owned API protocol files.

## Integration contract

1. Dispatcher resolves the project's actual working root and candidate file targets using platform-native filesystem APIs. A lexical Windows path alone is insufficient (junctions, reparse points, symlinks, 8.3 names, Unicode).
2. Authorization checks module enablement, user-approved accessible directories and conversation/task permissions **independently** of project membership and path reservation.
3. After authorization, normalize verified absolute targets, then call `TaskGate::start(task_id, scopes)` before invoking any mutating local tool. Reject conflicts; never silently bypass or reinterpret invalid scopes.
4. Keep `TaskLease` alive across the **entire** execution, including tool calls and verification. A cancellation must terminate or await outstanding writes before dropping the lease. Dropping it while detached writers still run is unsafe.
5. Persist task state transitions and execution receipts separately. `TaskGate` is in-memory and does not recover after restart. On restart, reconcile interrupted tasks and verify external side effects before retrying.
6. Use `TaskGate::execute` only for synchronous tool calls that finish before returning. Asynchronous tools must retain a lease until all operations have actually stopped.
7. All agents modifying the same filesystem need a shared registry. Separate processes require an OS-backed cross-process strategy before claiming safe parallel writes.

## Explicit limitations

- The parser currently rejects UNC, non-ASCII segments, relative paths, device paths and ambiguous Windows names. This is conservative but not a general-purpose Windows path resolver.
- A reserved scope is **not** a file permission or user approval.
- This is a coordination primitive, not a transaction, sandbox, audit log or Git merge/conflict resolver.
- No GitHub Actions should run before the team's budget pause ends; commits use `[skip ci]`.
