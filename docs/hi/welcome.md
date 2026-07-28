# Introduction

## Description

This folder is the second language in the starter, here to show how
multi-language docs work. Add a directory under `docs/` named for the language
code and the switcher in the sidebar picks it up automatically.

Each language keeps its own pages, its own `nav.md`, and — if you want it — its
own `site.md` to override the title, subtitle, or theme.

## Architecture

### Arch

```
docs/
  site.md      <- global settings
  en/          <- one folder per language
  hi/
```

### JSON

```json
{
  "languages": ["en", "hi"],
  "default": "en",
  "per_language_overrides": ["nav.md", "site.md"]
}
```

### Text

Translate the pages in this folder, or delete the folder entirely if you only
need one language. The default language is whichever sorts first.
