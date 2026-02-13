use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use actix_files::Files;
use actix_web::http::header::LOCATION;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use notify::{RecursiveMode, Result as NotifyResult, Watcher};
use pulldown_cmark::{html, Options, Parser as MdParser};
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

    #[arg(long, default_value = "Srotas Space")]
    site_title: String,
}

#[derive(Parser, Clone)]
struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    #[arg(long, default_value_t = 8088)]
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
}

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
            build_site(&args).context("build failed")?;
        }
        Commands::Serve(args) => {
            let site = build_site(&args.build).context("build failed")?;
            if args.watch {
                start_watcher(args.build.clone());
            }
            serve_site(args, site).await?;
        }
    }

    Ok(())
}

fn build_site(args: &BuildArgs) -> Result<SiteMeta> {
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
            let description_html = markdown_to_html(&desc_md);
            let architecture_html = markdown_to_html(&arch_md);
            let architecture_json_html = markdown_to_html(&json_md);
            let architecture_text_html = markdown_to_html(&text_md);
            let content_text = markdown_to_text(&expanded);
            let excerpt = content_text.chars().take(160).collect::<String>();

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
            ctx.insert("page_title", &page.title);
            ctx.insert("lang", &lang.code);
            ctx.insert("content_html", &content_html);
            ctx.insert("description_html", &description_html);
            ctx.insert("architecture_html", &architecture_html);
            ctx.insert("architecture_json_html", &architecture_json_html);
            ctx.insert("architecture_text_html", &architecture_text_html);
            ctx.insert("nav_groups", &nav_groups);
            ctx.insert("nav_pages", &lang.pages);
            ctx.insert("current_url", &page.url);
            ctx.insert("langs", &site.langs);
            ctx.insert("dev_reload", &false);

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

    let marker = args.out_dir.join(".docsgen");
    fs::write(marker, "managed by docsgen")?;

    Ok(site)
}

async fn serve_site(args: ServeArgs, site: SiteMeta) -> Result<()> {
    let out_dir = args.build.out_dir.clone();
    let default_lang = site.default_lang.clone();

    let bind_addr = format!("{}:{}", args.host, args.port);
    println!("Serving on http://{bind_addr}");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(default_lang.clone()))
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

        if source_rel == "template.md" {
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

fn markdown_to_html(md: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);

    let parser = MdParser::new_ext(md, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
        .replace("<h2>Description</h2>", "<h2 id=\"description\">Description</h2>")
        .replace("<h2>Architecture</h2>", "<h2 id=\"architecture\">Architecture</h2>")
}

fn split_sections(md: &str) -> (String, String, String, String) {
    let mut description = String::new();
    let mut architecture = String::new();
    let mut architecture_json = String::new();
    let mut architecture_text = String::new();

    let mut current: Option<&str> = None;
    let mut arch_mode: Option<&str> = None;
    for line in md.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("## Description") {
            current = Some("description");
            arch_mode = None;
            continue;
        }
        if trimmed.eq_ignore_ascii_case("## Architecture") {
            current = Some("architecture");
            arch_mode = None;
            continue;
        }
        if trimmed.eq_ignore_ascii_case("### Arch") {
            if current == Some("architecture") {
                arch_mode = Some("arch");
                continue;
            }
        }
        if trimmed.eq_ignore_ascii_case("### JSON") {
            if current == Some("architecture") {
                arch_mode = Some("json");
                continue;
            }
        }
        if trimmed.eq_ignore_ascii_case("### Text") {
            if current == Some("architecture") {
                arch_mode = Some("text");
                continue;
            }
        }
        if trimmed.starts_with("## ") {
            current = None;
            arch_mode = None;
        }

        match current {
            Some("description") => {
                description.push_str(line);
                description.push('\n');
            }
            Some("architecture") => {
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
            }
            _ => {}
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

fn start_watcher(args: BuildArgs) {
    std::thread::spawn(move || {
        if let Err(err) = watch_and_rebuild(args) {
            eprintln!("watcher error: {err}");
        }
    });
}

fn watch_and_rebuild(args: BuildArgs) -> NotifyResult<()> {
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
                    if let Err(err) = build_site(&args) {
                        eprintln!("rebuild failed: {err}");
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
                        _ => {}
                    }
                }
            }
        }
    }
    config
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
