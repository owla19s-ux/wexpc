Retest dispatch/lease regressions after applying the exact rustfmt diff from failed run 40527579681.
Run cargo fmt --all -- --check, strict workspace Clippy, and workspace tests.
Only formatting changes in dispatch.rs and dispatch_leases.rs; no runtime or permission changes.
