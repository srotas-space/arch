# Welcome

## Description

This stack is built for writing clean API docs without a backend.

## Architecture

### Arch

```
[Markdown] -> [Rust Generator] -> [Static HTML]
           |-> [Actix Dev Server] -> [Browser]
```

### JSON

```json
{
  "run_locally": ["npm install", "npm run build:css", "cargo run --manifest-path docsgen/Cargo.toml -- serve"],
  "build": "cargo run --manifest-path docsgen/Cargo.toml -- build",
  "output": "public/"
}
```

### Text

Run `npm install` and `npm run build:css` once to compile styles.
Start the dev server with `cargo run --manifest-path docsgen/Cargo.toml -- serve`.
To export a static site, run `cargo run --manifest-path docsgen/Cargo.toml -- build` — output goes to `public/`.
