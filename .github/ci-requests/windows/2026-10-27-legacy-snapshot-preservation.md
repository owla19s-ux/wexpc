Validate storage fix fc1f6f79ab832a231f96361409ee2b89214e8c35 on Windows.
The desktop service enables serde_json raw_value to preserve legacy opaque root JSON without losing history or rounding numbers.
Run the existing Windows workspace tests, resource/storage contracts and locked Tauri Host check. This is automated build/test evidence only, not installation or real service acceptance.
