RED regression rerun after applying rustfmt's array layout from run 40529226726. Production is unchanged.
Also verify opaque historical numbers are not rounded by preservation.
Run fmt, strict Clippy and workspace tests; the history-preservation assertions should fail before the storage fix.
