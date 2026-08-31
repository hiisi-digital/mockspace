# First-party lint presets

TOML files dropped here are embedded into `mockspace-rs` at build time and
served under the `mockspace::<name>` shorthand. Each `<name>.toml` becomes
`mockspace::<name>` at consumer sites.

The schema is `PresetFile` in `mockspace-config`. A minimal preset:

```toml
schema_version = "1.0"
name = "example"
primitive = "forbidden_imports"
description = "Example preset; demonstrates the schema."

[config]
forbidden = ["alloc::*"]
reason = "no-heap discipline"

[severity]
build = "error"

[scope]
# axes from ScopedLintConfig; optional
```

Conventions:

- File name (minus `.toml`) is the preset name. It must match the `name`
  field; the loader checks this at first access and surfaces a hard
  `ConfigError` on mismatch so the audit trail stays clean.
- The `primitive` field names the catalog kind the preset configures. It
  must match a registered catalog primitive at engine startup.
- The `extends` field chains to another preset (first-party or external)
  via the `<host>::<name>` shorthand.

The build.rs codegen pipeline lives at `../../build.rs` and emits an
`EMBEDDED_PRESET_TOML` slice consumed by `preset_source::FirstPartyPresetSource`.
