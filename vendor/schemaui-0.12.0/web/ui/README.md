# SchemaUI Web Workspace

This package hosts the offline React single-page application that ships with the
`schemaui` crate. The UI is bundled into a single `index.html` file (via
`vite-plugin-singlefile`) and embedded into the Rust binary through
`include_dir`, so no CDN access is required at runtime.

## Local development

```bash
cd web/ui
pnpm install         # once
pnpm dev             # launches Vite on http://127.0.0.1:5173
```

The layout mirrors the TUI: fixed header/footer, draggable tree/editor/preview
panes, syntax-highlighted previews, realtime JSON Schema validation, and a theme
toggle.

## Production build

```bash
pnpm run build:embedded
```

This runs `tsc -b` followed by `vite build`, emitting `../dist/index.html`.
Cargo’s `build.rs` automatically triggers `pnpm run build:embedded` unless
`SCHEMAUI_WEB_SKIP_BUILD=1`. Set `SCHEMAUI_WEB_FORCE_BUILD=1` to rebuild
unconditionally.

## Type-safe data contracts

The server layer derives TypeScript bindings via
[`ts-rs`](https://crates.io/crates/ts-rs). Generated files live in
`web/types/*.ts` and are addressable inside the SPA (and in third-party
frontends) through the alias `@schemaui/types/*`. Run

```bash
cargo test -p schemaui --lib --features web
```

whenever you modify the Rust data structures to refresh those bindings.

The helper module `src/types.ts` re-exports the generated interfaces while
replacing `serde_json::Value` with the richer `JsonValue` union the UI expects.

## Custom frontends

Consumers embedding `schemaui` in their own products can ship a fully custom
bundle by:

1. Implementing a frontend that talks to the documented HTTP endpoints
   (`/api/session`, `/api/validate`, `/api/preview`, etc.). Import the generated
   definitions from `web/types` to stay in sync with the server.
2. Pointing the library at their assets via
   `WebSessionBuilder::with_asset_provider` or `with_filesystem_assets`.

The CLI wrapper (`cargo run -p schemaui-cli -- web ...`) automatically falls
back to the embedded bundle if no override is provided.
