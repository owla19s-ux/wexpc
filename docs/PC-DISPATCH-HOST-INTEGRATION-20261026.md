# WEXPC 0.1.1 Dispatch Core / Host integration contract

Status: implementation boundary, 2026-10-26. This is not product scope expansion or a claim of Windows acceptance.

## Implemented on this branch

- `wex_core::dispatch::admit`: project, assignment, epoch and state eligibility.
- `validate_ticket`: prevents historical tickets from changing an unrelated or reassigned task.
- `wex_core::dispatch_leases::ResourceLeases`: atomic, process-local exclusive scope leases, released by RAII.
- `reserve`: combines admission and lease acquisition, then checks the ticket again.

These functions **do not grant permission**, invoke an Agent, or persist a task. In particular, a lease registry is not a Windows filesystem lock or a distributed scheduler. The current scope keys are opaque strings: the Host must canonicalize paths and resolve overlapping parent/child resources before leasing. Different strings referring to the same resource must not be treated as independent.

## Next Host transaction boundary

1. Load authoritative project/task assignment and revision from durable storage.
2. Validate project membership, role/duty, selected connection and current permission.
3. Normalize requested resource scopes and acquire the relevant process-local lease.
4. Recheck assignment epoch and task state; persist the Running transition and dispatch attempt ID with a compare-and-swap revision.
5. Only then invoke the selected Agent using its independent conversation and tool approvals.
6. Persist operation intent, outcome and receipt with the task/attempt identity. A success receipt is not automatic task completion; verifier/reviewer checks remain separate.
7. On uncertain external outcome, persist Blocked/OutcomeUnknown, never blindly replay writes.
8. Release lease on terminal attempt. After restart, treat interrupted work as unknown and reconcile external effects before any new dispatch.

## Verification status

Rust unit tests have been authored but not executed in this turn. Windows build, desktop UI, actual model calls, restart recovery and cross-process exclusion are not verified. GitHub Actions are intentionally not triggered. Keep version 0.1.1 and do not merge without integration review.
