use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::schemars::JsonSchema;
use rmcp::serde_json;
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::parser;
use crate::vault;

#[derive(Clone)]
pub struct Notal {
    vault_root: PathBuf,
    tool_router: ToolRouter<Self>,
}

impl Notal {
    pub fn new(vault_root: PathBuf) -> Self {
        let tool_router = Self::tool_router();
        Self { vault_root, tool_router }
    }
}

#[derive(Deserialize, JsonSchema)]
struct ReadNoteParams {
    /// Path relative to vault root (e.g. "Projects/my-note.md"). Auto-appends .md if missing.
    path: String,
    /// Maximum number of body lines to return. Omit for full content.
    max_lines: Option<usize>,
    /// When true, return only metadata (frontmatter, tags, links, title) with empty body.
    metadata_only: Option<bool>,
}

#[derive(Serialize)]
struct ReadNoteResult {
    path: String,
    frontmatter: Option<serde_json::Value>,
    body: String,
    tags: Vec<String>,
    links: Vec<LinkInfo>,
    title: Option<String>,
}

#[derive(Serialize)]
struct LinkInfo {
    target: String,
    heading: Option<String>,
    alias: Option<String>,
}

impl From<parser::WikiLink> for LinkInfo {
    fn from(l: parser::WikiLink) -> Self {
        Self { target: l.target, heading: l.heading, alias: l.alias }
    }
}

#[derive(Deserialize, JsonSchema)]
struct ListNotesParams {
    /// Subfolder to list (relative to vault root). Omit for entire vault.
    folder: Option<String>,
    /// Filter by inline tag (e.g. "rust" matches #rust).
    tag: Option<String>,
    /// Filter by frontmatter key=value (e.g. "status=active").
    frontmatter_filter: Option<String>,
}

#[derive(Serialize)]
struct NoteEntry {
    path: String,
    title: Option<String>,
    tags: Vec<String>,
}

#[derive(Serialize)]
struct ListNotesResult {
    notes: Vec<NoteEntry>,
    count: usize,
}

#[derive(Deserialize, JsonSchema)]
struct SearchNotesParams {
    /// Text or regex pattern to search for.
    query: String,
    /// Restrict search to a subfolder.
    folder: Option<String>,
    /// Context lines around each match (default: 1).
    context_lines: Option<usize>,
    /// Maximum number of matching files (default: 20).
    max_results: Option<usize>,
    /// Maximum matches to return per file (default: 10).
    max_matches_per_file: Option<usize>,
}

#[derive(Serialize)]
struct SearchNotesResult {
    results: Vec<SearchFileResult>,
    count: usize,
}

#[derive(Serialize)]
struct SearchFileResult {
    path: String,
    matches: Vec<MatchInfo>,
    /// True if more matches existed in this file but were not returned.
    truncated: bool,
}

#[derive(Serialize)]
struct MatchInfo {
    line_number: usize,
    line: String,
    context_before: Vec<String>,
    context_after: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct GetLinksParams {
    /// Path to the note (relative to vault root).
    path: String,
    /// Include backlinks from other notes (requires full vault scan). Default: true.
    backlinks: Option<bool>,
}

#[derive(Serialize)]
struct LinksResult {
    path: String,
    outgoing: Vec<LinkInfo>,
    backlinks: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct QueryFrontmatterParams {
    /// Frontmatter field to match (e.g. "status").
    key: String,
    /// Value to match. Omit to find all notes with the key present.
    value: Option<String>,
    /// Restrict to a subfolder.
    folder: Option<String>,
}

#[derive(Serialize)]
struct FrontmatterMatch {
    path: String,
    frontmatter: serde_json::Value,
}

#[derive(Serialize)]
struct QueryFrontmatterResult {
    matches: Vec<FrontmatterMatch>,
    count: usize,
}

#[derive(Deserialize, JsonSchema)]
struct WriteNoteParams {
    /// Path relative to vault root.
    path: String,
    /// Full note content (including frontmatter if desired).
    content: String,
    /// Overwrite if file exists (default: false).
    overwrite: Option<bool>,
}

#[derive(Serialize)]
struct WriteNoteResult {
    path: String,
    created: bool,
    bytes_written: usize,
}

/// Check whether a JSON value matches an expected string representation.
fn frontmatter_value_matches(field: &serde_json::Value, expected: &str) -> bool {
    field.as_str().is_some_and(|s| s == expected)
        || field.to_string().trim_matches('"') == expected
}

#[tool_router]
impl Notal {
    #[tool(description = "Read a note's content, frontmatter, tags, and wikilinks")]
    fn read_note(&self, Parameters(params): Parameters<ReadNoteParams>) -> Result<String, String> {
        let path = vault::resolve_path(&self.vault_root, &params.path).map_err(|e| e.to_string())?;
        let content = vault::read_note(&path).map_err(|e| e.to_string())?;
        let parsed = parser::parse_note(&content);

        let body = if params.metadata_only.unwrap_or(false) {
            String::new()
        } else if let Some(max) = params.max_lines {
            let lines: Vec<&str> = parsed.body.lines().collect();
            if lines.len() > max {
                format!("{}\n\n... ({} more lines)", lines[..max].join("\n"), lines.len() - max)
            } else {
                parsed.body
            }
        } else {
            parsed.body
        };

        let result = ReadNoteResult {
            path: vault::relative_path(&self.vault_root, &path),
            frontmatter: parsed.frontmatter,
            body,
            tags: parsed.tags,
            links: parsed.links.into_iter().map(LinkInfo::from).collect(),
            title: parsed.title,
        };
        serde_json::to_string(&result).map_err(|e| e.to_string())
    }

    #[tool(description = "List notes in the vault, optionally filtered by folder, tag, or frontmatter")]
    fn list_notes(&self, Parameters(params): Parameters<ListNotesParams>) -> Result<String, String> {
        let notes = vault::walk_notes(&self.vault_root, params.folder.as_deref());

        // Parse frontmatter filter if given: "key=value"
        let fm_filter = params.frontmatter_filter.as_ref().and_then(|f| {
            f.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))
        });

        let mut entries = Vec::new();
        for path in notes {
            let rel = vault::relative_path(&self.vault_root, &path);

            // If we need to filter, read and parse the note
            if params.tag.is_some() || fm_filter.is_some() {
                let content = match vault::read_note(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let parsed = parser::parse_note(&content);

                // Tag filter: check both inline tags and frontmatter tags
                if let Some(ref tag) = params.tag {
                    let fm_tags = parsed.frontmatter.as_ref()
                        .map(|fm| parser::frontmatter_tags(fm))
                        .unwrap_or_default();
                    if !parsed.tags.iter().any(|t| t == tag) && !fm_tags.iter().any(|t| t == tag) {
                        continue;
                    }
                }

                // Frontmatter filter
                if let Some((ref key, ref value)) = fm_filter {
                    match &parsed.frontmatter {
                        Some(fm) => {
                            if !fm.get(key).is_some_and(|v| frontmatter_value_matches(v, value)) {
                                continue;
                            }
                        }
                        None => continue,
                    }
                }

                entries.push(NoteEntry {
                    path: rel,
                    title: parsed.title.or_else(|| path.file_stem().map(|s| s.to_string_lossy().to_string())),
                    tags: parsed.tags,
                });
            } else {
                // Fast path: no filtering needed, extract title cheaply
                let title = path.file_stem().map(|s| s.to_string_lossy().to_string());
                entries.push(NoteEntry { path: rel, title, tags: Vec::new() });
            }
        }

        entries.sort_by(|a, b| a.path.cmp(&b.path));
        let count = entries.len();
        serde_json::to_string(&ListNotesResult { notes: entries, count }).map_err(|e| e.to_string())
    }

    #[tool(description = "Search notes by text or regex pattern, returning matching lines with context")]
    fn search_notes(&self, Parameters(params): Parameters<SearchNotesParams>) -> Result<String, String> {
        let re = regex::Regex::new(&params.query).unwrap_or_else(|_| {
            regex::Regex::new(&regex::escape(&params.query)).unwrap()
        });
        let ctx = params.context_lines.unwrap_or(1);
        let max = params.max_results.unwrap_or(20);

        let notes = vault::walk_notes(&self.vault_root, params.folder.as_deref());
        let mut results = Vec::new();

        for path in notes {
            if results.len() >= max { break; }

            let content = match vault::read_note(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let max_per_file = params.max_matches_per_file.unwrap_or(10);
            let lines: Vec<&str> = content.lines().collect();
            let mut matches = Vec::new();
            let mut truncated = false;

            for (i, line) in lines.iter().enumerate() {
                if matches.len() >= max_per_file {
                    truncated = true;
                    break;
                }
                if re.is_match(line) {
                    let start = i.saturating_sub(ctx);
                    let end = (i + ctx + 1).min(lines.len());

                    matches.push(MatchInfo {
                        line_number: i + 1,
                        line: line.to_string(),
                        context_before: lines[start..i].iter().map(|s| s.to_string()).collect(),
                        context_after: lines[i + 1..end].iter().map(|s| s.to_string()).collect(),
                    });
                }
            }

            if !matches.is_empty() {
                results.push(SearchFileResult {
                    path: vault::relative_path(&self.vault_root, &path),
                    matches,
                    truncated,
                });
            }
        }

        let count = results.len();
        serde_json::to_string(&SearchNotesResult { results, count }).map_err(|e| e.to_string())
    }

    #[tool(description = "Get outgoing wikilinks from a note and backlinks from other notes pointing to it")]
    fn get_links(&self, Parameters(params): Parameters<GetLinksParams>) -> Result<String, String> {
        let path = vault::resolve_path(&self.vault_root, &params.path).map_err(|e| e.to_string())?;
        let content = vault::read_note(&path).map_err(|e| e.to_string())?;

        let outgoing: Vec<LinkInfo> = parser::extract_wikilinks(&content)
            .into_iter()
            .map(LinkInfo::from)
            .collect();

        let backlinks = if params.backlinks.unwrap_or(true) {
            let note_stem = path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            let all_notes = vault::walk_notes(&self.vault_root, None);
            let mut backlinks = Vec::new();

            for other_path in all_notes {
                if other_path == path { continue; }
                let other_content = match vault::read_note(&other_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let links = parser::extract_wikilinks(&other_content);
                if links.iter().any(|l| l.target == note_stem) {
                    backlinks.push(vault::relative_path(&self.vault_root, &other_path));
                }
            }
            backlinks
        } else {
            Vec::new()
        };

        serde_json::to_string(&LinksResult {
            path: vault::relative_path(&self.vault_root, &path),
            outgoing,
            backlinks,
        }).map_err(|e| e.to_string())
    }

    #[tool(description = "Find notes matching a frontmatter field and optional value")]
    fn query_frontmatter(&self, Parameters(params): Parameters<QueryFrontmatterParams>) -> Result<String, String> {
        let notes = vault::walk_notes(&self.vault_root, params.folder.as_deref());
        let mut results = Vec::new();

        for path in notes {
            let content = match vault::read_note(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let parsed = parser::parse_note(&content);

            if let Some(ref fm) = parsed.frontmatter {
                if let Some(field_val) = fm.get(&params.key) {
                    let matches = match &params.value {
                        Some(v) => frontmatter_value_matches(field_val, v),
                        None => true, // key exists, no value filter
                    };
                    if matches {
                        results.push(FrontmatterMatch {
                            path: vault::relative_path(&self.vault_root, &path),
                            frontmatter: fm.clone(),
                        });
                    }
                }
            }
        }

        let count = results.len();
        serde_json::to_string(&QueryFrontmatterResult { matches: results, count }).map_err(|e| e.to_string())
    }

    #[tool(description = "Create or update a note in the vault")]
    fn write_note(&self, Parameters(params): Parameters<WriteNoteParams>) -> Result<String, String> {
        let path = vault::resolve_path(&self.vault_root, &params.path).map_err(|e| e.to_string())?;
        let existed = path.exists();
        let bytes = vault::write_note(&path, &params.content, params.overwrite.unwrap_or(false))
            .map_err(|e| e.to_string())?;

        serde_json::to_string(&WriteNoteResult {
            path: vault::relative_path(&self.vault_root, &path),
            created: !existed,
            bytes_written: bytes,
        }).map_err(|e| e.to_string())
    }
}

#[tool_handler]
impl ServerHandler for Notal {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Notal: Obsidian vault tools for reading, searching, and writing notes.")
            .with_server_info(rmcp::model::Implementation::new("notal", env!("CARGO_PKG_VERSION")))
    }
}
