# SchemaUI Type Bindings

This directory is populated automatically by `ts-rs` during
`cargo check
--features web` (or any other build that enables the `web`
feature). Each Rust data structure that powers the HTTP API and blueprint
metadata emits a matching TypeScript file here, e.g. `WebBlueprint.ts`,
`SessionResponse.ts`, etc.

Usage tips:

1. **In the bundled SPA** – import through the alias
   `@schemaui/types/<TypeName>` (see `web/ui/src/types.ts`). Vite is configured
   to resolve this alias and the TypeScript configs point to the same location.
2. **For custom frontends** – add `web/types` to your project (e.g. via a git
   submodule or a vendored copy) and reference the exact same files so your UI
   always stays in sync with the Rust backend.
3. **Regeneration** – run `cargo test -p schemaui --lib --features web` whenever
   you modify the Rust models. The generated tests created by the
   `#[ts(export)]` attributes will refresh this directory.

> Note: the files themselves are committed so that `npm run build` has immediate
> access to the bindings even before Cargo recompiles the crate.
