# Rust Docs Generator (Actix + Tailwind)

Markdown → static HTML docs with a Rust generator and Actix dev server. Supports multi‑language pages, split panels, and architecture tabs (Arch/JSON/Text).

## Quick start

## Fork and use

1) Fork this repository to your account.
2) Clone your fork locally:

```bash
git clone <your-fork-url>
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
- Build CSS in watch mode (optional):
  ```bash
  npm run dev:css
  ```
- Build static site:
  ```bash
  cargo run --manifest-path docsgen/Cargo.toml -- build
  ```
