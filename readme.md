# Simple way to create infra architecture documentation

Markdown → static HTML docs with a Rust generator and Actix dev server. Supports multi‑language pages, split panels, and architecture tabs (Arch/JSON/Text).

## Quick start

## Fork and use

1) Fork this repository to your account.
2) Clone your fork locally:

```bash
git clone https://github.com/srotas-space/arch.git
cd arch
```

3) Follow the Quick start steps below.

```bash
npm install
npm run build:css
cargo run --manifest-path docsgen/Cargo.toml -- serve
```

Open: `http://127.0.0.1:8088/`

## Build static HTML

```bash
cargo run --manifest-path docsgen/Cargo.toml -- build
```

Output is written to `public/`.

## Deploy (static hosting)

1) Build CSS and HTML:

```bash
npm run build:css
cargo run --manifest-path docsgen/Cargo.toml -- build
```

2) Upload the `public/` folder to your static host (S3, CloudFront, Netlify, Nginx, etc).

### Deploy to S3

```bash
aws s3 mb s3://YOUR_BUCKET
aws s3 sync public/ s3://YOUR_BUCKET --delete
aws s3 website s3://YOUR_BUCKET --index-document index.html --error-document index.html
```

### Deploy to CloudFront (with S3 origin)

1) Create an S3 bucket and upload `public/` (see above).
2) Create a CloudFront distribution with the S3 bucket as origin.
3) Set the default root object to `index.html`.
4) (Optional) Configure custom error responses to return `index.html` for 404s.

### Deploy to AWS Amplify

1) Create a new Amplify app connected to your repo.
2) Set build commands:

```bash
npm install
npm run build:css
cargo run --manifest-path docsgen/Cargo.toml -- build
```

3) Set the publish directory to `public/`.

### Deploy to Nginx

1) Copy the site to your server:

```bash
rsync -av public/ user@server:/var/www/docs/
```

2) Example Nginx server block:

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

### Deploy to Netlify

- Build command:
  ```bash
  npm install && npm run build:css && cargo run --manifest-path docsgen/Cargo.toml -- build
  ```
- Publish directory: `public/`

## Project structure

- `docs/en/*.md`, `docs/hi/*.md` — content sources.
- `docsgen/` — generator + Actix dev server.
- `docsgen/templates/page.html` — layout template.
- `assets/input.css` — Tailwind source CSS.
- `assets/app.css` — compiled CSS.
- `public/` — generated static site.

## Mandatory files (per language)

- `welcome.md` — used as the homepage (`/en/`, `/hi/`).
- `template.md` — optional but recommended for ordered includes.

## Rust + Actix setup (macOS / Ubuntu / Windows)

### macOS

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Ubuntu

```bash
sudo apt update
sudo apt install -y build-essential curl
curl https://sh.rustup.rs -sSf | sh
source "$HOME/.cargo/env"
```

### Windows

1) Install Rust via rustup: https://rustup.rs  
2) Restart your terminal, then verify:

```bash
rustc --version
cargo --version
```

## Node.js setup (macOS / Ubuntu / Windows)

### macOS

```bash
brew install node
```

### Ubuntu

```bash
sudo apt update
sudo apt install -y nodejs npm
```

### Windows

1) Install Node.js (LTS): https://nodejs.org  
2) Restart your terminal, then verify:

```bash
node --version
npm --version
```

## Writing pages

Each page should follow this structure to populate the split panels and tabs:

```md
# Page Title

## Description
Short overview and context.

## Architecture
### Arch
Text diagram or code block.

### JSON
```json
{ "sample": true }
```

### Text
Plain explanation.
```

If a page omits these headings, the content will still render; the architecture tabs may be empty.

## Include pages in a specific order (Markdown-only)

You can compose a page from multiple files using `@include:` directives. This lets you control order without changing Rust.

Example `docs/en/template.md`:

```md
# Architecture Overview

@include: 01-intro.md
@include: 02-architecture.md
@include: 03-pricing.md
@include: 04-mine.md
```

All `@include:` paths are relative to the language folder (e.g., `docs/en/`).

`welcome.md` is treated as the default page for that language and will be served at `/en/`. `template.md` remains a normal page.

## Testing / QA

- Verify the dev server:
  ```bash
  cargo run --manifest-path docsgen/Cargo.toml -- serve
  ```
- Check that `public/` has updated HTML after:
  ```bash
  cargo run --manifest-path docsgen/Cargo.toml -- build
  ```
- Rebuild CSS whenever you change styles:
  ```bash
  npm run build:css
  ```

## Where to change things

- **Layout / UI:** `docsgen/templates/page.html`
- **Styles:** `assets/input.css` (then run `npm run build:css`)
- **Generator logic:** `docsgen/src/main.rs`
- **Content:** `docs/<lang>/*.md`

## Common tasks

- Start dev server:
  ```bash
  cargo run --manifest-path docsgen/Cargo.toml -- serve
  ```
- Start dev server with watch (auto-build on .md changes):
  ```bash
  cargo run --manifest-path docsgen/Cargo.toml -- serve --watch
  ```
- Auto-build on Markdown changes (no server restart):
  ```bash
  ./scripts/watch-docs.sh
  ```
- Build CSS in watch mode (optional):
  ```bash
  npm run dev:css
  ```
- Build static site:
  ```bash
  cargo run --manifest-path docsgen/Cargo.toml -- build
  ```
