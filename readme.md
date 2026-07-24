# Arch — Markdown to static documentation

Write infrastructure and API docs in Markdown. A Rust generator compiles them into a clean, static HTML site with a split-panel layout, architecture tabs (Arch / JSON / Text), multi-language support, and full-text search. No backend, no database, no runtime.

---

## Quick start

**Prerequisites:** Rust (`rustup`) and Node.js.

```bash
# 1. Install CSS dependencies
npm install
npm run build:css

# 2. Start the dev server (auto-reloads on .md changes)
cargo run --manifest-path docsgen/Cargo.toml -- serve --watch
```

Open `http://127.0.0.1:8095/`

---

## Fork and use

1. Fork this repository to your account.
2. Clone your fork:

```bash
git clone https://github.com/srotas-space/arch.git
cd arch
```

3. Build and run:

```bash
cargo build --manifest-path docsgen/Cargo.toml
npm install && npm run build:css
cargo run --manifest-path docsgen/Cargo.toml -- serve --watch
```

---

## Build static HTML

```bash
cargo run --manifest-path docsgen/Cargo.toml -- build
```

Output is written to `public/`. The folder is self-contained — upload it to any static host.

---

## Deploy

### Push to deploy branch

The script `scripts/push-to-deploy-branch.sh` builds the site and force-pushes only the `public/` contents to a target branch (default: `deploy`). Use this for any static host that serves from a branch (Netlify, Cloudflare Pages, etc.).

```bash
# Build + push to deploy branch
./scripts/push-to-deploy-branch.sh

# Push to a different branch
./scripts/push-to-deploy-branch.sh staging

# Skip the build step (push current public/ as-is)
SKIP_BUILD=1 ./scripts/push-to-deploy-branch.sh
```

| Env var | Default | Description |
| --- | --- | --- |
| `DEPLOY_BRANCH` | `deploy` | Target branch to push to |
| `REMOTE` | `origin` | Git remote name |
| `SKIP_BUILD` | `0` | Set to `1` to skip CSS + site build |

---



### Netlify

- Build command: `npm install && npm run build:css && cargo run --manifest-path docsgen/Cargo.toml -- build`
- Publish directory: `public/`

### AWS S3

```bash
npm run build:css
cargo run --manifest-path docsgen/Cargo.toml -- build

aws s3 mb s3://YOUR_BUCKET
aws s3 sync public/ s3://YOUR_BUCKET --delete
aws s3 website s3://YOUR_BUCKET --index-document index.html --error-document index.html
```

### CloudFront (S3 origin)

1. Create an S3 bucket and sync `public/` (see above).
2. Create a CloudFront distribution pointed at the S3 bucket.
3. Set the default root object to `index.html`.
4. Optional: configure custom error responses to return `index.html` for 404s.

### AWS Amplify

1. Connect your fork to a new Amplify app.
2. Set build commands:

```bash
npm install
npm run build:css
cargo run --manifest-path docsgen/Cargo.toml -- build
```

3. Publish directory: `public/`

### Nginx

```bash
rsync -av public/ user@server:/var/www/docs/
```

```nginx
server {
  listen 80;
  server_name docs.example.com;
  root /var/www/docs;
  index index.html;

  location / {
    try_files $uri /index.html;
  }
}
```

---

## Project structure

```
arch/
├── docs/
│   ├── site.md              # Global site settings
│   ├── en/
│   │   ├── welcome.md       # Homepage for /en/
│   │   ├── nav.md           # Sidebar navigation groups
│   │   └── *.md             # Content pages
│   └── hi/
│       ├── welcome.md
│       └── *.md
├── docsgen/
│   ├── src/main.rs          # Generator + Actix dev server
│   └── templates/page.html  # HTML layout template
├── assets/
│   ├── input.css            # Tailwind source
│   └── app.css              # Compiled CSS (committed)
└── public/                  # Generated static site
```

---

## Writing pages

Every page follows this structure:

```md
# Page Title

## Description
Short overview shown in the left panel.

## Architecture

### Arch
ASCII diagram or freeform text.

### JSON
\`\`\`json
{ "key": "value" }
\`\`\`

### Text
Plain prose explanation shown in the Text tab.
```

The generator maps these headings to the split-panel UI automatically. Omitting `## Architecture` renders the description full-width.

---

## Site settings

Create `docs/site.md` for global settings, or `docs/<lang>/site.md` for per-language overrides:

```md
title: Srotas Space
subtitle: Infra Arch
logo: /assets/logo.png
footer: Arch by Srotas Space
theme: violet
```

---

## Themes

### How to change the theme

**Step 1** — open `docs/site.md` and set the `theme:` line to any theme from the table below:

```md
title: Srotas Space
subtitle: Infra Arch
logo: /assets/logo.png
footer: Arch by Srotas Space
theme: ocean
```

**Step 2** — rebuild:

```bash
cargo run --manifest-path docsgen/Cargo.toml -- build
```

That's it. If the dev server is already running with `--watch`, skip step 2 entirely — saving `site.md` rebuilds and reloads the browser automatically:

```bash
cargo run --manifest-path docsgen/Cargo.toml -- serve --watch
```

> You do **not** need to run `npm run build:css` when switching themes. All palettes are already compiled into `assets/app.css`; the theme is selected at build time via a `data-theme` attribute on `<html>`. CSS only needs recompiling if you edit `assets/input.css` itself.

If `theme:` is missing, the site uses `violet`. A name that isn't in the table falls back to `violet` and prints a warning during the build:

```
warning: unknown theme 'blue' in site.md; using 'violet'. Available: violet, ocean, forest, ember, slate
```

### Available themes

| Value | Look |
| --- | --- |
| `violet` | Purple → amber gradient (default) |
| `ocean` | Deep blue → teal |
| `forest` | Green → gold |
| `ember` | Rust → orange |
| `slate` | Neutral grey → sky |

### Light and dark

Every theme ships a **light and a dark palette**. The dark one is applied automatically when the visitor's operating system is set to dark mode. There is no toggle to configure and no JavaScript involved — it is pure CSS (`prefers-color-scheme`), so it works on any static host.

To preview the dark palette, switch your OS appearance to dark and reload the page.

### Different theme per language

`theme:` in `docs/site.md` applies to the whole site. To theme one language differently, set it in that language's own settings file — `docs/hi/site.md`:

```md
theme: forest
```

Now the Hindi pages render in `forest` while everything else keeps the global theme. Per-language values override the global one.

### Adding your own theme

**Step 1** — in `assets/input.css`, copy an existing `[data-theme="..."]` block and rename the selector. Copy **both** halves: the light block near the top, and its counterpart inside the `@media (prefers-color-scheme: dark)` section.

**Step 2** — register the name in `docsgen/src/main.rs` so it passes validation:

```rust
const THEMES: [&str; 5] = ["violet", "ocean", "forest", "ember", "slate"];
```

Add your name to the list and bump the array length.

**Step 3** — recompile the CSS, then build:

```bash
npm run build:css
cargo run --manifest-path docsgen/Cargo.toml -- build
```

Note that `serve --watch` only watches `docs/` and `docsgen/templates/` — it does not watch `assets/`. While tuning a palette, run `npm run dev:css` in a second terminal to recompile on save.

Themes are plain CSS custom properties, so a palette is just a list of values:

| Token | Controls |
| --- | --- |
| `--bg`, `--bg-dot` | Page background and dot grid |
| `--surface`, `--surface-2` | Card backgrounds, table headers, chips |
| `--border` | All hairline borders |
| `--fg`, `--fg-muted`, `--fg-subtle` | Headings, body text, meta text |
| `--accent-from`, `--accent-mid`, `--accent-to` | Gradient for titles, active tabs, tags |
| `--accent-soft`, `--accent-ink` | Inline `code` background and text |
| `--code-bg`, `--code-fg` | Fenced code blocks |
| `--sidebar-grad` | Sidebar gradient |
| `--sidebar-pill-ink` | Text on the active language pill |
| `--search-panel-bg` | Search results dropdown |
| `--selection` | Text selection highlight |

Components reference these tokens, so changing a palette never means touching component rules.

---

## Navigation groups

Create `docs/<lang>/nav.md` to define a two-level grouped sidebar:

```md
[Overview]
- welcome.md
- getting-started.md

[Architecture]
- architecture.md

[Infrastructure]
- network.md
- compute.md
- data.md

[Operations]
- monitoring.md
- incident-response.md
```

Groups render as collapsible sections. Without a `nav.md`, pages are listed flat in filesystem order.

---

## Composing pages with includes

Use `@include:` to assemble a page from multiple files without touching Rust:

```md
# Architecture Overview

@include: 01-intro.md
@include: 02-services.md
@include: 03-costs.md
```

Paths are relative to the language folder (e.g., `docs/en/`).

---

## Where to change things

| What | Where |
| --- | --- |
| Layout / HTML structure | `docsgen/templates/page.html` |
| Theme | `theme:` in `docs/site.md` |
| Theme palettes | `assets/input.css` → run `npm run build:css` |
| Styles | `assets/input.css` → run `npm run build:css` |
| Generator logic | `docsgen/src/main.rs` |
| Content | `docs/<lang>/*.md` |
| Logo | Replace `assets/logo.png` (recommended 24×24 px) |
| Search index | Auto-generated at `public/search.json` on build |

---

## Common commands

| Task | Command |
| --- | --- |
| Dev server | `cargo run --manifest-path docsgen/Cargo.toml -- serve` |
| Dev server + watch | `cargo run --manifest-path docsgen/Cargo.toml -- serve --watch` |
| Build static site | `cargo run --manifest-path docsgen/Cargo.toml -- build` |
| Compile CSS | `npm run build:css` |
| Watch CSS | `npm run dev:css` |
| Run tests | `cargo test --manifest-path docsgen/Cargo.toml` |

---

## Installing prerequisites

### Rust

**macOS**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Ubuntu**
```bash
sudo apt update && sudo apt install -y build-essential curl
curl https://sh.rustup.rs -sSf | sh
source "$HOME/.cargo/env"
```

**Windows** — install via `https://rustup.rs`, then restart your terminal.

### Node.js

**macOS** — `brew install node`

**Ubuntu** — `sudo apt install -y nodejs npm`

**Windows** — install LTS from `https://nodejs.org`, then restart your terminal.
