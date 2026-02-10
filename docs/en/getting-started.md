# Getting Started

This stack is built for writing clean API docs without a backend.

## Run locally

```bash
npm install
npm run build:css

cargo run --manifest-path docsgen/Cargo.toml -- serve
```

## Build static HTML

```bash
cargo run --manifest-path docsgen/Cargo.toml -- build
```

The generator writes static files into `public/`.
