use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::Duration;

use actix_files::Files;
use actix_web::http::header::LOCATION;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use notify::{RecursiveMode, Result as NotifyResult, Watcher};
use pulldown_cmark::{html, CowStr, Event, Options, Parser as MdParser, Tag, TagEnd};
use serde::Serialize;
use tera::{Context as TeraContext, Tera};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "docsgen", version, about = "Rust docs generator with Actix dev server")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Build(BuildArgs),
    Serve(ServeArgs),
}

#[derive(Parser, Clone)]
struct BuildArgs {
    #[arg(long, default_value = "docs")]
    docs_dir: PathBuf,

    #[arg(long, default_value = "public")]
    out_dir: PathBuf,

    #[arg(long, default_value = "assets")]
    assets_dir: PathBuf,

    #[arg(long, default_value = "docsgen/templates")]
    templates_dir: PathBuf,

    #[arg(long, default_value = "Docs")]
    site_title: String,
}

#[derive(Parser, Clone)]
struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    #[arg(long, default_value_t = 8095)]
    port: u16,

    #[arg(long, default_value_t = false)]
    watch: bool,

    #[command(flatten)]
    build: BuildArgs,
}

#[derive(Clone, Debug, Serialize)]
struct PageMeta {
    title: String,
    url: String,
    rel_slug: String,
    source_rel: String,
}

#[derive(Clone, Debug, Serialize)]
struct NavItem {
    title: String,
    url: String,
}

#[derive(Clone, Debug, Serialize)]
struct NavGroup {
    title: String,
    items: Vec<NavItem>,
    open: bool,
}

#[derive(Clone, Debug, Serialize)]
struct LangMeta {
    code: String,
    pages: Vec<PageMeta>,
}

#[derive(Clone, Debug)]
struct SiteMeta {
    langs: Vec<LangMeta>,
    default_lang: String,
}

#[derive(Clone, Debug, Default)]
struct SiteConfig {
    title: Option<String>,
    logo: Option<String>,
    footer: Option<String>,
    subtitle: Option<String>,
    theme: Option<String>,
    api_base: Option<String>,
}

/// One entry in the "On this page" list, built from the `##`/`###` headings of
/// the Description section.
#[derive(Clone, Debug, Serialize)]
struct TocItem {
    level: u8,
    title: String,
    id: String,
}

/// A single card in the JSON tab: a request, a response, or a curl sample.
/// `code_html` is already highlighted and HTML-escaped.
#[derive(Clone, Debug, Serialize)]
struct ApiBlock {
    kind: String,
    label: String,
    method: Option<String>,
    path: Option<String>,
    status: Option<String>,
    status_class: String,
    note: Option<String>,
    lang: String,
    code_html: String,
    generated: bool,
    /// Unhighlighted source, kept so a curl sample can be derived from a
    /// request block. Not needed by the template.
    #[serde(skip)]
    raw: String,
}

/// Theme presets defined in `assets/input.css`. Adding a theme means adding a
/// `[data-theme="name"]` block there (light + dark) and a name here.
const THEMES: [&str; 6] = ["violet", "indigo", "ocean", "forest", "ember", "slate"];
const DEFAULT_THEME: &str = "violet";

/// Host used when synthesising a curl sample from a request block. Override in
/// `site.md` with `api_base:`.
const DEFAULT_API_BASE: &str = "https://api.example.com";

const HTTP_METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

#[derive(Clone, Debug, Serialize)]
struct SearchEntry {
    lang: String,
    title: String,
    url: String,
    excerpt: String,
    content: String,
}

#[actix_web::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build(args) => {
            build_site(&args, false).context("build failed")?;
        }
        Commands::Serve(args) => {
            let reload_state = Arc::new(AtomicU64::new(now_millis()));
            let site = build_site(&args.build, args.watch).context("build failed")?;
            reload_state.store(now_millis(), Ordering::Release);
            if args.watch {
                start_watcher(args.build.clone(), reload_state.clone());
            }
            serve_site(args, site, reload_state).await?;
        }
    }

    Ok(())
}

fn build_site(args: &BuildArgs, dev_reload: bool) -> Result<SiteMeta> {
    if !args.docs_dir.exists() {
        return Err(anyhow!("docs dir not found: {}", args.docs_dir.display()));
    }

    let templates_glob = format!("{}/**/*.html", args.templates_dir.display());
    let tera = Tera::new(&templates_glob)
        .with_context(|| format!("failed to load templates: {templates_glob}"))?;

    prepare_output_dir(&args.out_dir)?;

    let site = collect_site_meta(&args.docs_dir)?;
    let mut search_entries: Vec<SearchEntry> = Vec::new();
    for lang in &site.langs {
        for page in &lang.pages {
            let nav_groups =
                load_nav_groups(&args.docs_dir.join(&lang.code), &lang.pages, &page.url);
            let site_config = load_site_config(&args.docs_dir, &lang.code);
            let md_path = args
                .docs_dir
                .join(&lang.code)
                .join(&page.source_rel);
            if !md_path.exists() {
                continue;
            }
            let markdown = fs::read_to_string(&md_path)
                .with_context(|| format!("failed to read {}", md_path.display()))?;
            let expanded = expand_includes(&markdown, md_path.parent().unwrap_or(&args.docs_dir))
                .with_context(|| format!("failed to expand includes in {}", md_path.display()))?;
            let (desc_md, arch_md, json_md, text_md) = split_sections(&expanded);
            let content_html = markdown_to_html(&expanded);
            let (description_html, toc) = markdown_to_html_with_toc(&desc_md);
            let architecture_html = markdown_to_html(&arch_md);
            let architecture_json_html = markdown_to_html(&json_md);
            let architecture_text_html = markdown_to_html(&text_md);
            let content_text = markdown_to_text(&expanded);
            let excerpt = content_text.chars().take(160).collect::<String>();

            let api_base = site_config
                .api_base
                .as_deref()
                .unwrap_or(DEFAULT_API_BASE)
                .trim_end_matches('/')
                .to_string();
            let api_blocks = build_api_blocks(&json_md, &api_base);
            let (prev_page, next_page) = neighbour_pages(&nav_groups, &lang.pages, &page.url);
            let breadcrumb = breadcrumb_for(&nav_groups, &page.url);

            let mut ctx = TeraContext::new();
            let title = site_config
                .title
                .as_deref()
                .unwrap_or(&args.site_title);
            ctx.insert("site_title", &title);
            if let Some(logo) = &site_config.logo {
                ctx.insert("site_logo", logo);
            }
            if let Some(footer) = &site_config.footer {
                ctx.insert("site_footer", footer);
            }
            if let Some(subtitle) = &site_config.subtitle {
                ctx.insert("site_subtitle", subtitle);
            }
            ctx.insert("site_theme", &resolve_theme(site_config.theme.as_deref()));
            ctx.insert("page_title", &page.title);
            ctx.insert("lang", &lang.code);
            ctx.insert("content_html", &content_html);
            ctx.insert("description_html", &description_html);
            ctx.insert("architecture_html", &architecture_html);
            ctx.insert("architecture_json_html", &architecture_json_html);
            ctx.insert("architecture_text_html", &architecture_text_html);
            ctx.insert("api_blocks", &api_blocks);
            ctx.insert("toc", &toc);
            ctx.insert("nav_groups", &nav_groups);
            ctx.insert("nav_pages", &lang.pages);
            ctx.insert("current_url", &page.url);
            ctx.insert("langs", &site.langs);
            ctx.insert("dev_reload", &dev_reload);
            if let Some(prev) = &prev_page {
                ctx.insert("prev_page", prev);
            }
            if let Some(next) = &next_page {
                ctx.insert("next_page", next);
            }
            if let Some(crumb) = &breadcrumb {
                ctx.insert("breadcrumb", crumb);
            }

            let rendered = tera
                .render("page.html", &ctx)
                .context("failed to render template")?;

            let out_path = output_path_for(&args.out_dir, &lang.code, &page.rel_slug);
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(&out_path, rendered)
                .with_context(|| format!("failed to write {}", out_path.display()))?;

            search_entries.push(SearchEntry {
                lang: lang.code.clone(),
                title: page.title.clone(),
                url: page.url.clone(),
                excerpt,
                content: content_text,
            });
        }
    }

    copy_assets(&args.assets_dir, &args.out_dir.join("assets"))?;
    write_search_index(&args.out_dir, &search_entries)?;
    write_root_index(&args.out_dir, &site.default_lang)?;

    let marker = args.out_dir.join(".docsgen");
    fs::write(marker, "managed by docsgen")?;

    Ok(site)
}

fn write_root_index(out_dir: &Path, default_lang: &str) -> Result<()> {
    let target = format!("/{default_lang}/");
    let html = format!(
        "<!doctype html>\n\
<html lang=\"en\">\n\
<head>\n\
  <meta charset=\"utf-8\">\n\
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
  <title>Redirecting...</title>\n\
  <meta http-equiv=\"refresh\" content=\"0; url={target}\">\n\
  <script>window.location.replace('{target}');</script>\n\
</head>\n\
<body>\n\
  <p>Redirecting to <a href=\"{target}\">{target}</a></p>\n\
</body>\n\
</html>\n"
    );
    fs::write(out_dir.join("index.html"), html)
        .with_context(|| format!("failed to write {}/index.html", out_dir.display()))?;
    Ok(())
}

async fn serve_site(
    args: ServeArgs,
    site: SiteMeta,
    reload_state: Arc<AtomicU64>,
) -> Result<()> {
    let out_dir = args.build.out_dir.clone();
    let default_lang = site.default_lang.clone();

    let bind_addr = format!("{}:{}", args.host, args.port);
    println!("Serving on http://{bind_addr}");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(default_lang.clone()))
            .app_data(web::Data::new(reload_state.clone()))
            .route("/__reload", web::get().to(reload_poll))
            .route("/", web::get().to(root_redirect))
            .route("/{lang}", web::get().to(lang_redirect))
            .service(Files::new("/", &out_dir).index_file("index.html"))
    })
    .bind(bind_addr)?
    .run()
    .await?;

    Ok(())
}

async fn root_redirect(default_lang: web::Data<String>) -> impl Responder {
    HttpResponse::Found()
        .append_header((LOCATION, format!("/{}/", default_lang.get_ref())))
        .finish()
}

async fn lang_redirect(path: web::Path<String>) -> impl Responder {
    let lang = path.into_inner();
    HttpResponse::Found()
        .append_header((LOCATION, format!("/{}/", lang)))
        .finish()
}

async fn reload_poll(state: web::Data<Arc<AtomicU64>>) -> impl Responder {
    let value = state.load(Ordering::Acquire);
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(value.to_string())
}

fn prepare_output_dir(out_dir: &Path) -> Result<()> {
    let marker = out_dir.join(".docsgen");
    if out_dir.exists() && marker.exists() {
        fs::remove_dir_all(out_dir)
            .with_context(|| format!("failed to clean {}", out_dir.display()))?;
    }
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    Ok(())
}

fn collect_site_meta(docs_dir: &Path) -> Result<SiteMeta> {
    let mut langs = Vec::new();
    for entry in fs::read_dir(docs_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let lang_code = entry.file_name().to_string_lossy().to_string();
        let pages = collect_pages_for_lang(&entry.path(), &lang_code)?;
        if !pages.is_empty() {
            langs.push(LangMeta { code: lang_code, pages });
        }
    }

    if langs.is_empty() {
        return Err(anyhow!("no languages found under {}", docs_dir.display()));
    }

    langs.sort_by(|a, b| a.code.cmp(&b.code));
    let default_lang = langs.first().unwrap().code.clone();

    Ok(SiteMeta { langs, default_lang })
}

fn collect_pages_for_lang(lang_dir: &Path, lang_code: &str) -> Result<Vec<PageMeta>> {
    let mut pages = Vec::new();
    let include_order = load_include_order(lang_dir).unwrap_or_default();
    for entry in WalkDir::new(lang_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let rel_path = entry.path().strip_prefix(lang_dir)?;
        let source_rel = rel_path.to_string_lossy().replace('\\', "/");
        let mut rel_slug = path_without_extension(rel_path);
        if rel_slug == "welcome" {
            rel_slug = "index".to_string();
        }
        let markdown = fs::read_to_string(entry.path())
            .with_context(|| format!("failed to read {}", entry.path().display()))?;
        let expanded = expand_includes(&markdown, lang_dir)
            .with_context(|| format!("failed to expand includes in {}", entry.path().display()))?;
        let title = extract_title(&expanded)
            .unwrap_or_else(|| title_from_slug(&rel_slug));
        let url = url_for(lang_code, &rel_slug);

        // Config files, not content: they drive the sidebar and site settings
        // and must not be published as pages of their own.
        if matches!(source_rel.as_str(), "template.md" | "nav.md" | "site.md") {
            continue;
        }

        pages.push(PageMeta {
            title,
            url,
            rel_slug,
            source_rel,
        });
    }

    pages.sort_by(|a, b| {
        let a_idx = order_index(&include_order, &a.source_rel, &a.rel_slug);
        let b_idx = order_index(&include_order, &b.source_rel, &b.rel_slug);
        a_idx
            .cmp(&b_idx)
            .then_with(|| a.rel_slug.cmp(&b.rel_slug))
    });
    Ok(pages)
}

fn path_without_extension(path: &Path) -> String {
    let mut rel = path.to_path_buf();
    rel.set_extension("");
    rel.to_string_lossy().replace('\\', "/")
}

fn url_for(lang: &str, rel_slug: &str) -> String {
    if rel_slug == "index" {
        format!("/{lang}/")
    } else {
        format!("/{lang}/{rel_slug}")
    }
}

fn output_path_for(out_dir: &Path, lang: &str, rel_slug: &str) -> PathBuf {
    if rel_slug == "index" {
        out_dir.join(lang).join("index.html")
    } else {
        out_dir.join(lang).join(rel_slug).join("index.html")
    }
}

fn extract_title(md: &str) -> Option<String> {
    for line in md.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            return Some(trimmed.trim_start_matches("# ").trim().to_string());
        }
    }
    None
}

fn title_from_slug(slug: &str) -> String {
    let last = slug.rsplit('/').next().unwrap_or(slug);
    let mut words = Vec::new();
    for part in last.split(|c| c == '-' || c == '_') {
        if part.is_empty() {
            continue;
        }
        let mut chars = part.chars();
        let title = match chars.next() {
            Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
            None => continue,
        };
        words.push(title);
    }
    if words.is_empty() {
        "Untitled".to_string()
    } else {
        words.join(" ")
    }
}

fn md_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    options
}

fn markdown_to_html(md: &str) -> String {
    markdown_to_html_with_toc(md).0
}

/// Render markdown, giving every heading a stable `id` plus a hover anchor, and
/// returning the `##`/`###` headings so the sidebar can show an "On this page"
/// list. Ids are assigned from the event stream rather than by rewriting the
/// output, so raw HTML headings inside the page (the `<h3>` in the stack cards,
/// for instance) are left alone and never shift the numbering.
fn markdown_to_html_with_toc(md: &str) -> (String, Vec<TocItem>) {
    let events: Vec<Event> = MdParser::new_ext(md, md_options()).collect();

    let mut toc: Vec<TocItem> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Event> = Vec::with_capacity(events.len() + 16);

    let mut i = 0;
    while i < events.len() {
        let Event::Start(Tag::Heading {
            level,
            id,
            classes,
            attrs,
        }) = &events[i]
        else {
            out.push(events[i].clone());
            i += 1;
            continue;
        };

        let mut end = i + 1;
        let mut text = String::new();
        while end < events.len() {
            match &events[end] {
                Event::End(TagEnd::Heading(_)) => break,
                Event::Text(t) | Event::Code(t) => text.push_str(t),
                _ => {}
            }
            end += 1;
        }

        let slug = match id {
            Some(existing) => existing.to_string(),
            None => {
                let base = slugify(&text);
                let count = seen.entry(base.clone()).or_insert(0);
                *count += 1;
                if *count == 1 {
                    base
                } else {
                    format!("{base}-{count}")
                }
            }
        };

        let level_num = *level as u8;
        if (level_num == 2 || level_num == 3) && !text.trim().is_empty() {
            toc.push(TocItem {
                level: level_num,
                title: text.trim().to_string(),
                id: slug.clone(),
            });
        }

        out.push(Event::Start(Tag::Heading {
            level: *level,
            id: Some(CowStr::from(slug.clone())),
            classes: classes.clone(),
            attrs: attrs.clone(),
        }));
        for event in &events[i + 1..end] {
            out.push(event.clone());
        }
        out.push(Event::Html(CowStr::from(format!(
            "<a class=\"heading-anchor\" href=\"#{slug}\" aria-label=\"Link to this section\">#</a>"
        ))));
        if end < events.len() {
            out.push(events[end].clone());
        }
        i = end + 1;
    }

    let mut html_output = String::new();
    html::push_html(&mut html_output, out.into_iter());
    (html_output, toc)
}

fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.extend(ch.to_lowercase());
        } else if !out.is_empty() {
            pending_dash = true;
        }
    }
    if out.is_empty() {
        "section".to_string()
    } else {
        out
    }
}

fn split_sections(md: &str) -> (String, String, String, String) {
    let mut description = String::new();
    let mut architecture = String::new();
    let mut architecture_json = String::new();
    let mut architecture_text = String::new();

    // Everything is prose by default. `## Architecture` switches to the
    // reference column and any *other* `## ` heading switches back, so the
    // sections an author writes after it (cost tables, stack grids) stay on the
    // page instead of being dropped on the floor. Section markers are ignored
    // inside fenced code so a `## ` line in a sample never splits the page.
    let mut current = "description";
    let mut arch_mode: Option<&str> = None;
    let mut seen_title = false;
    let mut in_fence = false;

    for line in md.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("```") {
            in_fence = !in_fence;
        }

        if !in_fence {
            if !seen_title && trimmed.starts_with("# ") {
                seen_title = true;
                continue;
            }
            if trimmed.eq_ignore_ascii_case("## Description") {
                current = "description";
                arch_mode = None;
                continue;
            }
            if trimmed.eq_ignore_ascii_case("## Architecture") {
                current = "architecture";
                arch_mode = None;
                continue;
            }
            if current == "architecture" {
                if trimmed.eq_ignore_ascii_case("### Arch") {
                    arch_mode = Some("arch");
                    continue;
                }
                if trimmed.eq_ignore_ascii_case("### JSON") {
                    arch_mode = Some("json");
                    continue;
                }
                if trimmed.eq_ignore_ascii_case("### Text") {
                    arch_mode = Some("text");
                    continue;
                }
            }
            if trimmed.starts_with("## ") {
                // The heading itself belongs to the prose column, so no
                // `continue` here — it falls through and is written out.
                current = "description";
                arch_mode = None;
            }
        }

        if current == "architecture" {
            match arch_mode {
                Some("json") => {
                    architecture_json.push_str(line);
                    architecture_json.push('\n');
                }
                Some("text") => {
                    architecture_text.push_str(line);
                    architecture_text.push('\n');
                }
                _ => {
                    architecture.push_str(line);
                    architecture.push('\n');
                }
            }
        } else {
            description.push_str(line);
            description.push('\n');
        }
    }

    let has_arch = !architecture.trim().is_empty()
        || !architecture_json.trim().is_empty()
        || !architecture_text.trim().is_empty();

    if description.trim().is_empty() && !has_arch {
        return (md.to_string(), String::new(), String::new(), String::new());
    }

    (description, architecture, architecture_json, architecture_text)
}

fn start_watcher(args: BuildArgs, reload_state: Arc<AtomicU64>) {
    std::thread::spawn(move || {
        if let Err(err) = watch_and_rebuild(args, reload_state) {
            eprintln!("watcher error: {err}");
        }
    });
}

fn watch_and_rebuild(args: BuildArgs, reload_state: Arc<AtomicU64>) -> NotifyResult<()> {
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    watcher.watch(&args.docs_dir, RecursiveMode::Recursive)?;
    watcher.watch(&args.templates_dir, RecursiveMode::Recursive)?;

    let mut pending = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(_) => pending = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if pending {
                    if let Err(err) = build_site(&args, true) {
                        eprintln!("rebuild failed: {err}");
                    } else {
                        reload_state.store(now_millis(), Ordering::Release);
                    }
                    pending = false;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn load_nav_groups(lang_dir: &Path, pages: &[PageMeta], current_url: &str) -> Vec<NavGroup> {
    let nav_path = lang_dir.join("nav.md");
    if !nav_path.exists() {
        return Vec::new();
    }

    let content = match fs::read_to_string(&nav_path) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };

    let mut page_map = std::collections::HashMap::new();
    for page in pages {
        page_map.insert(page.source_rel.clone(), page);
        page_map.insert(page.rel_slug.clone(), page);
    }

    let mut groups: Vec<NavGroup> = Vec::new();
    let mut current = NavGroup {
        title: "General".to_string(),
        items: Vec::new(),
        open: false,
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() > 2 {
            if !current.items.is_empty() {
                groups.push(current);
            }
            current = NavGroup {
                title: trimmed.trim_start_matches('[').trim_end_matches(']').trim().to_string(),
                items: Vec::new(),
                open: false,
            };
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix('-').or_else(|| trimmed.strip_prefix('*')) {
            let mut item = rest.trim().to_string();
            if item.is_empty() {
                continue;
            }
            if !item.ends_with(".md") {
                item.push_str(".md");
            }
            let page = page_map
                .get(&item)
                .or_else(|| page_map.get(item.trim_end_matches(".md")));
            if let Some(page) = page {
                let is_active = page.url == current_url;
                current.items.push(NavItem {
                    title: page.title.clone(),
                    url: page.url.clone(),
                });
                if is_active {
                    current.open = true;
                }
            }
        }
    }

    if !current.items.is_empty() {
        groups.push(current);
    }

    groups
}

fn load_include_order(lang_dir: &Path) -> Result<Vec<String>> {
    let template = lang_dir.join("template.md");
    if !template.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&template)?;
    let mut order = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@include:") {
            let rel = rest.trim();
            if !rel.is_empty() {
                order.push(rel.replace('\\', "/"));
            }
        }
    }
    Ok(order)
}

fn load_site_config(docs_dir: &Path, lang: &str) -> SiteConfig {
    let mut config = SiteConfig::default();
    let global = docs_dir.join("site.md");
    let lang_specific = docs_dir.join(lang).join("site.md");

    for path in [global, lang_specific] {
        if !path.exists() {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.is_empty() {
                    continue;
                }
                if let Some((key, value)) = trimmed.split_once(':') {
                    let key = key.trim();
                    let value = value.trim().trim_matches('"');
                    match key {
                        "title" => config.title = Some(value.to_string()),
                        "logo" => config.logo = Some(value.to_string()),
                        "footer" => config.footer = Some(value.to_string()),
                        "subtitle" => config.subtitle = Some(value.to_string()),
                        "theme" => config.theme = Some(value.to_lowercase()),
                        "api_base" => config.api_base = Some(value.to_string()),
                        _ => {}
                    }
                }
            }
        }
    }
    config
}

/// Map the `theme:` value from site.md to a known preset, falling back to the
/// default so a typo yields a styled site plus one warning rather than a
/// page with no palette at all.
fn resolve_theme(requested: Option<&str>) -> String {
    let Some(name) = requested.map(str::trim).filter(|s| !s.is_empty()) else {
        return DEFAULT_THEME.to_string();
    };
    if THEMES.contains(&name) {
        return name.to_string();
    }

    // load_site_config runs once per page, so warn only once per bad value.
    static WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let warned = WARNED.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(mut seen) = warned.lock() {
        if seen.insert(name.to_string()) {
            eprintln!(
                "warning: unknown theme '{name}' in site.md; using '{DEFAULT_THEME}'. Available: {}",
                THEMES.join(", ")
            );
        }
    }
    DEFAULT_THEME.to_string()
}

fn write_search_index(out_dir: &Path, entries: &[SearchEntry]) -> Result<()> {
    let path = out_dir.join("search.json");
    let json = serde_json::to_string(entries)?;
    fs::write(path, json)?;
    Ok(())
}

fn markdown_to_text(md: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for line in md.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if trimmed.starts_with("@include:") {
            continue;
        }
        let cleaned = trimmed
            .trim_start_matches('#')
            .trim_start_matches('*')
            .trim_start_matches('-')
            .trim();
        let cleaned = cleaned.replace('`', "");
        if !cleaned.is_empty() {
            out.push_str(&cleaned);
            out.push(' ');
        }
    }
    out
}

fn order_index(include_order: &[String], source_rel: &str, rel_slug: &str) -> usize {
    if rel_slug == "index" {
        return 0;
    }
    if source_rel == "template.md" {
        return 1;
    }
    if let Some(pos) = include_order
        .iter()
        .position(|item| item == source_rel)
    {
        return pos + 2;
    }
    usize::MAX
}

fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn expand_includes(md: &str, base_dir: &Path) -> Result<String> {
    expand_includes_inner(md, base_dir, 0)
}

fn expand_includes_inner(md: &str, base_dir: &Path, depth: usize) -> Result<String> {
    if depth > 5 {
        return Err(anyhow!("include depth exceeded"));
    }
    let mut out = String::new();
    for line in md.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@include:") {
            let rel = rest.trim();
            if rel.is_empty() {
                continue;
            }
            let target = base_dir.join(rel);
            let included = fs::read_to_string(&target)
                .with_context(|| format!("failed to read include {}", target.display()))?;
            let expanded = expand_includes_inner(&included, base_dir, depth + 1)?;
            out.push_str(&expanded);
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

/// Previous/next page in reading order — the flattened `nav.md` order when one
/// exists, otherwise the sidebar's own page order.
fn neighbour_pages(
    groups: &[NavGroup],
    pages: &[PageMeta],
    current_url: &str,
) -> (Option<NavItem>, Option<NavItem>) {
    let ordered: Vec<NavItem> = if groups.is_empty() {
        pages
            .iter()
            .map(|page| NavItem {
                title: page.title.clone(),
                url: page.url.clone(),
            })
            .collect()
    } else {
        groups.iter().flat_map(|g| g.items.iter().cloned()).collect()
    };

    let Some(idx) = ordered.iter().position(|item| item.url == current_url) else {
        return (None, None);
    };

    let prev = if idx > 0 {
        ordered.get(idx - 1).cloned()
    } else {
        None
    };
    (prev, ordered.get(idx + 1).cloned())
}

fn breadcrumb_for(groups: &[NavGroup], current_url: &str) -> Option<String> {
    groups
        .iter()
        .find(|group| group.items.iter().any(|item| item.url == current_url))
        .map(|group| group.title.clone())
}

// ─── API blocks ──────────────────────────────────────────────────────────────

/// Turn the `### JSON` section into request / response / curl cards.
///
/// Preferred form is one `#### ` subheading per block (`#### Request POST /v1/x`,
/// `#### Response 200`, `#### cURL`). Pages written before that existed — a
/// single JSON object with `request` / `response` / `*_error` keys — are split
/// on those top-level keys instead, so they render as separate cards untouched.
/// Anything else returns empty and the caller falls back to plain markdown.
fn build_api_blocks(json_md: &str, api_base: &str) -> Vec<ApiBlock> {
    let mut blocks = parse_api_blocks(json_md).unwrap_or_else(|| legacy_api_blocks(json_md));
    if blocks.is_empty() {
        return blocks;
    }

    // A request that names a method and path is enough to write the curl call
    // for the author; an explicit `#### cURL` block always wins.
    if !blocks.iter().any(|block| block.kind == "curl") {
        if let Some(idx) = blocks.iter().position(|block| block.kind == "request") {
            if let Some(curl) = synth_curl(&blocks[idx].raw, api_base) {
                let block = build_block(
                    "curl", "cURL", None, None, None, None, "bash", &curl, true,
                );
                blocks.insert(idx + 1, block);
            }
        }
    }

    blocks
}

/// Returns `None` when the section has no `#### ` subheadings at all, which is
/// how the caller knows to try the legacy split instead.
fn parse_api_blocks(md: &str) -> Option<Vec<ApiBlock>> {
    struct Pending {
        heading: String,
        note: Option<String>,
        lang: String,
        code: String,
    }

    let mut pending: Vec<Pending> = Vec::new();
    let mut saw_heading = false;
    let mut heading: Option<String> = None;
    let mut note: Vec<String> = Vec::new();
    let mut fence: Option<(String, String, Vec<String>)> = None;

    for line in md.lines() {
        let trimmed = line.trim();

        if let Some((marker, lang, lines)) = fence.as_mut() {
            let closes = trimmed.starts_with(marker.as_str())
                && trimmed.trim_end_matches('`').is_empty();
            if closes {
                pending.push(Pending {
                    heading: heading.take().unwrap_or_default(),
                    note: if note.is_empty() {
                        None
                    } else {
                        Some(note.join(" "))
                    },
                    lang: lang.clone(),
                    code: lines.join("\n"),
                });
                note.clear();
                fence = None;
            } else {
                lines.push(line.to_string());
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("#### ") {
            saw_heading = true;
            heading = Some(rest.trim().to_string());
            note.clear();
            continue;
        }

        if trimmed.starts_with("```") {
            let marker: String = trimmed.chars().take_while(|c| *c == '`').collect();
            let lang = trimmed[marker.len()..].trim().to_lowercase();
            fence = Some((marker, lang, Vec::new()));
            continue;
        }

        if heading.is_some() && !trimmed.is_empty() {
            note.push(trimmed.to_string());
        }
    }

    if !saw_heading {
        return None;
    }

    let mut blocks = Vec::new();
    for item in pending {
        let (kind, label, mut method, mut path, mut status) = parse_api_heading(&item.heading);
        if kind == "request" {
            method = method.or_else(|| {
                top_level_field(&item.code, "method").map(|value| value.to_uppercase())
            });
            path = path.or_else(|| top_level_field(&item.code, "path"));
        }
        if kind == "response" {
            status = status.or_else(|| {
                top_level_field(&item.code, "status")
                    .filter(|s| s.len() == 3 && s.chars().all(|c| c.is_ascii_digit()))
            });
        }
        blocks.push(build_block(
            &kind, &label, method, path, status, item.note, &item.lang, &item.code, false,
        ));
    }

    Some(blocks)
}

fn legacy_api_blocks(md: &str) -> Vec<ApiBlock> {
    let Some(code) = first_json_fence(md) else {
        return Vec::new();
    };
    let pairs = split_top_level_json_object(&code);
    let recognised = pairs.iter().any(|(key, _)| {
        let key = key.to_lowercase();
        key.contains("request") || key.contains("response") || key.contains("curl")
    });
    if !recognised {
        return Vec::new();
    }

    let mut blocks = Vec::new();
    for (key, value) in pairs {
        let lower = key.to_lowercase();
        let kind = if lower.contains("curl") {
            "curl"
        } else if lower.contains("request") {
            "request"
        } else if lower.contains("response") || lower.contains("error") {
            "response"
        } else {
            "other"
        };

        let body = dedent_body(&value);
        let (method, path) = if kind == "request" {
            (
                top_level_field(&body, "method").map(|m| m.to_uppercase()),
                top_level_field(&body, "path"),
            )
        } else {
            (None, None)
        };
        let status = if kind == "response" {
            top_level_field(&body, "status")
                .filter(|s| s.len() == 3 && s.chars().all(|c| c.is_ascii_digit()))
        } else {
            None
        };

        let lang = if kind == "curl" { "bash" } else { "json" };
        blocks.push(build_block(
            kind,
            &humanize_key(&key),
            method,
            path,
            status,
            None,
            lang,
            &body,
            false,
        ));
    }
    blocks
}

#[allow(clippy::too_many_arguments)]
fn build_block(
    kind: &str,
    label: &str,
    method: Option<String>,
    path: Option<String>,
    status: Option<String>,
    note: Option<String>,
    lang: &str,
    code: &str,
    generated: bool,
) -> ApiBlock {
    let code = code.trim_matches('\n').to_string();
    let lang = if lang.is_empty() {
        let head = code.trim_start();
        if head.starts_with('{') || head.starts_with('[') {
            "json"
        } else {
            "text"
        }
    } else {
        lang
    };

    let code_html = match lang {
        "json" => highlight_json(&code),
        "bash" | "sh" | "shell" | "curl" | "console" => highlight_shell(&code),
        _ => escape_html(&code),
    };

    ApiBlock {
        kind: kind.to_string(),
        label: label.to_string(),
        method,
        path,
        status_class: status.as_deref().map(status_class).unwrap_or_default(),
        status,
        note,
        lang: lang.to_string(),
        code_html,
        generated,
        raw: code,
    }
}

fn status_class(status: &str) -> String {
    match status.chars().next() {
        Some('2') => "ok",
        Some('3') => "info",
        Some('4') => "warn",
        Some('5') => "err",
        _ => "",
    }
    .to_string()
}

/// `Response 403 — Agent suspended` → response / "Agent suspended" / 403.
fn parse_api_heading(
    text: &str,
) -> (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let mut tokens = text.split_whitespace();
    let Some(first) = tokens.next() else {
        return ("other".into(), "Details".into(), None, None, None);
    };

    let (kind, default_label) = match first.trim_end_matches(':').to_lowercase().as_str() {
        "request" | "req" => ("request", "Request"),
        "response" | "resp" => ("response", "Response"),
        "error" | "errors" => ("response", "Error"),
        "curl" | "bash" | "shell" => ("curl", "cURL"),
        _ => ("other", ""),
    };
    if kind == "other" {
        return ("other".into(), text.trim().to_string(), None, None, None);
    }

    let mut method = None;
    let mut path = None;
    let mut status = None;
    let mut rest: Vec<&str> = Vec::new();

    for token in tokens {
        let upper = token.to_uppercase();
        if method.is_none() && HTTP_METHODS.contains(&upper.as_str()) {
            method = Some(upper);
        } else if status.is_none()
            && token.len() == 3
            && token.chars().all(|c| c.is_ascii_digit())
        {
            status = Some(token.to_string());
        } else if path.is_none() && token.starts_with('/') {
            path = Some(token.to_string());
        } else {
            rest.push(token);
        }
    }

    let label = rest
        .join(" ")
        .trim_matches(|c: char| {
            c == '—' || c == '–' || c == '-' || c == '·' || c == ':' || c.is_whitespace()
        })
        .to_string();
    let label = if label.is_empty() {
        default_label.to_string()
    } else {
        label
    };

    (kind.to_string(), label, method, path, status)
}

fn first_json_fence(md: &str) -> Option<String> {
    let mut fence: Option<(String, Vec<String>)> = None;
    for line in md.lines() {
        let trimmed = line.trim();
        if let Some((marker, lines)) = fence.as_mut() {
            if trimmed.starts_with(marker.as_str()) && trimmed.trim_end_matches('`').is_empty() {
                return Some(lines.join("\n"));
            }
            lines.push(line.to_string());
            continue;
        }
        if trimmed.starts_with("```") {
            let marker: String = trimmed.chars().take_while(|c| *c == '`').collect();
            let lang = trimmed[marker.len()..].trim().to_lowercase();
            if lang.is_empty() || lang == "json" {
                fence = Some((marker, Vec::new()));
            }
        }
    }
    fence.map(|(_, lines)| lines.join("\n"))
}

/// Split a JSON object into its top-level `(key, raw value)` pairs, keeping the
/// author's own formatting and key order. Re-serialising through serde_json
/// would sort keys and reflow the text, which is exactly what a docs page must
/// not do to a hand-written example.
fn split_top_level_json_object(src: &str) -> Vec<(String, String)> {
    let bytes = src.as_bytes();
    let mut pairs = Vec::new();
    let mut i = 0;

    while i < bytes.len() && bytes[i] != b'{' {
        if !bytes[i].is_ascii_whitespace() {
            return pairs;
        }
        i += 1;
    }
    if i >= bytes.len() {
        return pairs;
    }
    i += 1;

    loop {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'"' {
            break;
        }

        let key_start = i + 1;
        i += 1;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                break;
            }
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key = src[key_start..i].to_string();
        i += 1;

        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b':' {
            break;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }

        let value_start = i;
        let mut depth = 0usize;
        let mut in_string = false;
        while i < bytes.len() {
            let byte = bytes[i];
            if in_string {
                if byte == b'\\' {
                    i += 2;
                    continue;
                }
                if byte == b'"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            match byte {
                b'"' => {
                    in_string = true;
                    i += 1;
                }
                b'{' | b'[' => {
                    depth += 1;
                    i += 1;
                }
                b'}' | b']' => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    i += 1;
                }
                b',' if depth == 0 => break,
                _ => i += 1,
            }
        }

        if value_start >= i {
            break;
        }
        pairs.push((key, src[value_start..i].trim_end().to_string()));
    }

    pairs
}

fn top_level_field(raw: &str, key: &str) -> Option<String> {
    split_top_level_json_object(raw)
        .into_iter()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, value)| unquote(&value))
}

fn unquote(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else {
        trimmed.to_string()
    }
}

/// Strip the indentation a nested value carried from its parent object, leaving
/// the first line (which starts right after `"key": `) alone.
fn dedent_body(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    if lines.len() < 2 {
        return src.trim().to_string();
    }

    let indent = lines[1..]
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);

    let mut out = String::from(lines[0].trim_end());
    for line in &lines[1..] {
        out.push('\n');
        if line.trim().is_empty() {
            continue;
        }
        let cut = indent.min(line.len() - line.trim_start().len());
        out.push_str(line[cut..].trim_end());
    }
    out
}

fn humanize_key(key: &str) -> String {
    key.split(|c: char| c == '_' || c == '-' || c == ' ')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build a runnable curl call from a request block shaped like
/// `{ "method": ..., "path": ..., "headers": {...}, "body": {...} }`.
/// Returns `None` when there is no path to call.
fn synth_curl(request_raw: &str, api_base: &str) -> Option<String> {
    let fields = split_top_level_json_object(request_raw);
    if fields.is_empty() {
        return None;
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };

    let path = field("path").or_else(|| field("url")).map(|v| unquote(&v))?;
    if path.is_empty() {
        return None;
    }
    let method = field("method")
        .map(|v| unquote(&v).to_uppercase())
        .unwrap_or_else(|| "GET".to_string());

    let url = if path.starts_with("http://") || path.starts_with("https://") {
        path
    } else if path.starts_with('/') {
        format!("{api_base}{path}")
    } else {
        format!("{api_base}/{path}")
    };

    let body = field("body").or_else(|| field("payload"));

    let mut headers: Vec<String> = Vec::new();
    if let Some(raw) = field("headers") {
        for (key, value) in split_top_level_json_object(&raw) {
            headers.push(format!("{key}: {}", unquote(&value)));
        }
    }
    if body.is_some()
        && !headers
            .iter()
            .any(|header| header.to_lowercase().starts_with("content-type:"))
    {
        headers.push("Content-Type: application/json".to_string());
    }

    let mut out = format!("curl -X {method} {url}");
    for header in headers {
        out.push_str(" \\\n  -H \"");
        out.push_str(&header);
        out.push('"');
    }
    if let Some(body) = body {
        let body = dedent_body(&body);
        let indented = body
            .lines()
            .enumerate()
            .map(|(idx, line)| {
                if idx == 0 {
                    line.to_string()
                } else {
                    format!("  {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        out.push_str(" \\\n  -d '");
        out.push_str(&indented);
        out.push('\'');
    }

    Some(out)
}

// ─── Highlighting ────────────────────────────────────────────────────────────

fn escape_html(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for ch in src.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Minimal JSON tokeniser. Doing this at build time keeps the published site
/// free of a client-side highlighter while still colouring keys, strings,
/// numbers and literals distinctly.
fn highlight_json(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len() * 2);
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '"' => {
                let start = i;
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\\' {
                        i = (i + 2).min(chars.len());
                        continue;
                    }
                    if chars[i] == '"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                let literal: String = chars[start..i].iter().collect();

                let mut lookahead = i;
                while lookahead < chars.len() && chars[lookahead].is_whitespace() {
                    lookahead += 1;
                }
                let class = if lookahead < chars.len() && chars[lookahead] == ':' {
                    "tok-key"
                } else {
                    "tok-str"
                };
                out.push_str(&format!(
                    "<span class=\"{class}\">{}</span>",
                    escape_html(&literal)
                ));
            }
            '-' | '0'..='9' => {
                let start = i;
                i += 1;
                while i < chars.len()
                    && (chars[i].is_ascii_digit()
                        || matches!(chars[i], '.' | 'e' | 'E' | '+' | '-'))
                {
                    i += 1;
                }
                let literal: String = chars[start..i].iter().collect();
                out.push_str(&format!("<span class=\"tok-num\">{literal}</span>"));
            }
            't' | 'f' | 'n' => {
                let rest: String = chars[i..].iter().collect();
                match ["true", "false", "null"]
                    .iter()
                    .find(|word| rest.starts_with(**word))
                {
                    Some(word) => {
                        out.push_str(&format!("<span class=\"tok-lit\">{word}</span>"));
                        i += word.chars().count();
                    }
                    None => {
                        out.push(ch);
                        i += 1;
                    }
                }
            }
            '{' | '}' | '[' | ']' | ',' | ':' => {
                out.push_str(&format!("<span class=\"tok-punct\">{ch}</span>"));
                i += 1;
            }
            _ => {
                out.push_str(&escape_html(&ch.to_string()));
                i += 1;
            }
        }
    }

    out
}

/// Enough shell awareness to make a curl sample readable: the command itself,
/// its flags, quoted strings and the URL.
fn highlight_shell(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len() * 2);
    let mut i = 0;
    let mut is_first_word = true;

    while i < chars.len() {
        let ch = chars[i];

        if ch.is_whitespace() {
            out.push(ch);
            i += 1;
            continue;
        }

        if ch == '\'' || ch == '"' {
            let quote = ch;
            let start = i;
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i = (i + 2).min(chars.len());
                    continue;
                }
                if chars[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let literal: String = chars[start..i].iter().collect();
            out.push_str(&format!(
                "<span class=\"tok-str\">{}</span>",
                escape_html(&literal)
            ));
            is_first_word = false;
            continue;
        }

        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '\'' && chars[i] != '"' {
            i += 1;
        }
        let token: String = chars[start..i].iter().collect();
        let escaped = escape_html(&token);

        if is_first_word {
            out.push_str(&format!("<span class=\"tok-cmd\">{escaped}</span>"));
            is_first_word = false;
        } else if token.starts_with('-') {
            out.push_str(&format!("<span class=\"tok-flag\">{escaped}</span>"));
        } else if token.starts_with("http://") || token.starts_with("https://") {
            out.push_str(&format!("<span class=\"tok-url\">{escaped}</span>"));
        } else if token == "\\" {
            out.push_str(&format!("<span class=\"tok-punct\">{escaped}</span>"));
        } else {
            out.push_str(&escaped);
        }
    }

    out
}

fn copy_assets(src_dir: &Path, dest_dir: &Path) -> Result<()> {
    if !src_dir.exists() {
        return Ok(());
    }
    fs::create_dir_all(dest_dir)
        .with_context(|| format!("failed to create {}", dest_dir.display()))?;

    for entry in WalkDir::new(src_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        let rel = path.strip_prefix(src_dir)?;
        let target = dest_dir.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("failed to create {}", target.display()))?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(path, &target)
                .with_context(|| format!("failed to copy {}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_keep_content_written_after_architecture() {
        let md = "# Title\n\n## Description\n\nIntro.\n\n## Architecture\n\n### Arch\n\ndiagram\n\n### Text\n\nprose\n\n## Cost breakdown\n\n| a | b |\n";
        let (description, arch, _json, text) = split_sections(md);
        assert!(description.contains("Intro."));
        assert!(description.contains("## Cost breakdown"), "trailing section dropped");
        assert!(description.contains("| a | b |"));
        assert!(arch.contains("diagram"));
        assert!(text.contains("prose"));
        assert!(!description.contains("# Title"), "page h1 duplicated into prose");
    }

    #[test]
    fn section_markers_inside_code_fences_are_ignored() {
        let md = "## Description\n\n```\n## Architecture\n```\n\nstill prose\n";
        let (description, arch, _, _) = split_sections(md);
        assert!(description.contains("still prose"));
        assert!(arch.trim().is_empty());
    }

    #[test]
    fn api_heading_extracts_verb_path_status_and_title() {
        let (kind, label, method, path, status) = parse_api_heading("Request POST /v1/resources");
        assert_eq!((kind.as_str(), label.as_str()), ("request", "Request"));
        assert_eq!(method.as_deref(), Some("POST"));
        assert_eq!(path.as_deref(), Some("/v1/resources"));
        assert_eq!(status, None);

        let (kind, label, _, _, status) = parse_api_heading("Response 403 — Agent suspended");
        assert_eq!(kind, "response");
        assert_eq!(label, "Agent suspended");
        assert_eq!(status.as_deref(), Some("403"));

        let (kind, label, ..) = parse_api_heading("cURL");
        assert_eq!((kind.as_str(), label.as_str()), ("curl", "cURL"));

        let (kind, label, ..) = parse_api_heading("Webhook payload");
        assert_eq!((kind.as_str(), label.as_str()), ("other", "Webhook payload"));
    }

    #[test]
    fn top_level_split_preserves_key_order_and_formatting() {
        let src = "{\n  \"b\": {\n    \"nested\": \"x, y\"\n  },\n  \"a\": [1, 2],\n  \"c\": \"done\"\n}";
        let pairs = split_top_level_json_object(src);
        let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["b", "a", "c"]);
        assert!(pairs[0].1.contains("\"nested\": \"x, y\""), "comma inside a string split the value");
        assert_eq!(pairs[1].1, "[1, 2]");
    }

    #[test]
    fn legacy_single_object_splits_into_cards() {
        let md = "```json\n{\n  \"request\": {\n    \"method\": \"POST\",\n    \"path\": \"/v1/x\"\n  },\n  \"response\": { \"ok\": true },\n  \"not_active_error\": { \"status\": 403 }\n}\n```\n";
        let blocks = build_api_blocks(md, "https://api.test");
        let kinds: Vec<&str> = blocks.iter().map(|b| b.kind.as_str()).collect();
        assert_eq!(kinds, vec!["request", "curl", "response", "response"]);
        assert_eq!(blocks[0].method.as_deref(), Some("POST"));
        assert_eq!(blocks[3].label, "Not Active Error");
        assert_eq!(blocks[3].status.as_deref(), Some("403"));
        assert_eq!(blocks[3].status_class, "warn");
        assert!(blocks[1].generated);
    }

    #[test]
    fn plain_json_object_is_left_to_the_markdown_renderer() {
        let md = "```json\n{ \"metrics\": \"Prometheus\" }\n```\n";
        assert!(build_api_blocks(md, "https://api.test").is_empty());
    }

    #[test]
    fn authored_curl_block_suppresses_the_generated_one() {
        let md = "#### Request POST /v1/x\n\n```json\n{ \"method\": \"POST\", \"path\": \"/v1/x\" }\n```\n\n#### cURL\n\n```bash\ncurl mine\n```\n";
        let blocks = build_api_blocks(md, "https://api.test");
        assert_eq!(blocks.len(), 2);
        assert!(!blocks.iter().any(|b| b.generated));
    }

    #[test]
    fn generated_curl_carries_headers_and_body() {
        let request = "{\n  \"method\": \"post\",\n  \"path\": \"/v1/x\",\n  \"headers\": { \"APIKEY\": \"k\" },\n  \"body\": { \"a\": 1 }\n}";
        let curl = synth_curl(request, "https://api.test").expect("curl");
        assert!(curl.starts_with("curl -X POST https://api.test/v1/x"));
        assert!(curl.contains("-H \"APIKEY: k\""));
        assert!(curl.contains("-H \"Content-Type: application/json\""), "json body without content-type");
        assert!(curl.contains("-d '{ \"a\": 1 }'"));

        assert!(synth_curl("{ \"method\": \"GET\" }", "https://api.test").is_none(), "no path should mean no curl");
    }

    #[test]
    fn json_highlighting_separates_keys_from_values_and_escapes_html() {
        let out = highlight_json("{ \"k\": \"<b>\", \"n\": 4, \"t\": true }");
        assert!(out.contains("<span class=\"tok-key\">\"k\"</span>"));
        assert!(out.contains("<span class=\"tok-str\">\"&lt;b&gt;\"</span>"));
        assert!(out.contains("<span class=\"tok-num\">4</span>"));
        assert!(out.contains("<span class=\"tok-lit\">true</span>"));
    }

    #[test]
    fn slugs_are_unique_within_a_page() {
        let (_, toc) = markdown_to_html_with_toc("## Setup\n\ntext\n\n## Setup\n\nmore\n");
        let ids: Vec<&str> = toc.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, vec!["setup", "setup-2"]);
    }
}
