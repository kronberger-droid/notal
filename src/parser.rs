use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize)]
pub struct WikiLink {
    pub target: String,
    pub heading: Option<String>,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParsedNote {
    pub frontmatter: Option<serde_json::Value>,
    pub body: String,
    pub tags: Vec<String>,
    pub links: Vec<WikiLink>,
    pub title: Option<String>,
}

static WIKILINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[\[([^\]|#]+)(?:#([^\]|]+))?(?:\|([^\]]+))?\]\]").unwrap()
});

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s(])#([a-zA-Z][\w/-]*)").unwrap()
});

static TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^#\s+(.+)$").unwrap()
});

pub fn parse_frontmatter(content: &str) -> (Option<serde_yaml::Value>, &str) {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return (None, content);
    }

    let after_opening = if content.starts_with("---\r\n") { 5 } else { 4 };
    let rest = &content[after_opening..];

    let close = rest.find("\n---\n")
        .map(|i| (i, i + 5))
        .or_else(|| rest.find("\n---\r\n").map(|i| (i, i + 6)))
        .or_else(|| {
            // Handle case where --- is at the very end of file
            if rest.ends_with("\n---") {
                let i = rest.len() - 3;
                Some((i - 1, rest.len()))
            } else {
                None
            }
        });

    match close {
        Some((yaml_end, body_start)) => {
            let yaml_str = &rest[..yaml_end + 1]; // include the newline before ---
            let parsed = serde_yaml::from_str(yaml_str).ok();
            let body = &rest[body_start..];
            (parsed, body)
        }
        None => (None, content),
    }
}

/// Iterate over non-code-block lines in markdown content.
fn body_lines(content: &str) -> impl Iterator<Item = &str> {
    let mut in_code_block = false;
    content.lines().filter(move |line| {
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            return false;
        }
        !in_code_block
    })
}

pub fn extract_wikilinks(content: &str) -> Vec<WikiLink> {
    let mut links = Vec::new();
    for line in body_lines(content) {
        for cap in WIKILINK_RE.captures_iter(line) {
            links.push(WikiLink {
                target: cap[1].trim().to_string(),
                heading: cap.get(2).map(|m| m.as_str().trim().to_string()),
                alias: cap.get(3).map(|m| m.as_str().trim().to_string()),
            });
        }
    }
    links
}

pub fn extract_tags(content: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for line in body_lines(content) {
        for cap in TAG_RE.captures_iter(line) {
            let tag = cap[1].to_string();
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
    }
    tags
}

pub fn extract_title(content: &str) -> Option<String> {
    TITLE_RE.captures(content).map(|cap| cap[1].trim().to_string())
}

pub fn parse_note(content: &str) -> ParsedNote {
    let (frontmatter, body) = parse_frontmatter(content);
    let frontmatter_json = frontmatter.and_then(|v| serde_json::to_value(v).ok());
    let tags = extract_tags(body);
    let links = extract_wikilinks(body);
    let title = extract_title(body);

    ParsedNote {
        frontmatter: frontmatter_json,
        body: body.to_string(),
        tags,
        links,
        title,
    }
}

/// Extract tags from YAML frontmatter value (handles both arrays and strings)
pub fn frontmatter_tags(fm: &serde_json::Value) -> Vec<String> {
    match fm.get("tags") {
        Some(serde_json::Value::Array(arr)) => {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        }
        Some(serde_json::Value::String(s)) => {
            s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontmatter_parsing() {
        let content = "---\ntitle: Test Note\nstatus: active\ntags:\n  - rust\n  - mcp\n---\n# Hello\nBody text.";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_some());
        assert!(body.contains("# Hello"));
        assert!(body.contains("Body text."));
    }

    #[test]
    fn test_no_frontmatter() {
        let content = "# Just a heading\nSome text.";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn test_wikilinks() {
        let content = "Link to [[Note A]] and [[Note B#section|alias here]].";
        let links = extract_wikilinks(content);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "Note A");
        assert_eq!(links[1].target, "Note B");
        assert_eq!(links[1].heading.as_deref(), Some("section"));
        assert_eq!(links[1].alias.as_deref(), Some("alias here"));
    }

    #[test]
    fn test_wikilinks_in_code_block_ignored() {
        let content = "Before\n```\n[[Not A Link]]\n```\n[[Real Link]]";
        let links = extract_wikilinks(content);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "Real Link");
    }

    #[test]
    fn test_tags() {
        let content = "Some text #rust and #tools/mcp here.\n#another";
        let tags = extract_tags(content);
        assert!(tags.contains(&"rust".to_string()));
        assert!(tags.contains(&"tools/mcp".to_string()));
        assert!(tags.contains(&"another".to_string()));
    }

    #[test]
    fn test_tags_not_in_headings() {
        // Tags at line start need a space before # — headings like "# Title" won't match
        // because the regex requires ^|[\s(] before #
        let content = "# Heading\nText #real-tag";
        let tags = extract_tags(content);
        // "Heading" should not be a tag, "real-tag" should be
        assert!(!tags.iter().any(|t| t.contains("Heading")));
        assert!(tags.contains(&"real-tag".to_string()));
    }

    #[test]
    fn test_title_extraction() {
        let content = "Some preamble\n# My Title\nBody text.";
        assert_eq!(extract_title(content).as_deref(), Some("My Title"));
    }

    #[test]
    fn test_parse_note_full() {
        let content = "---\ntype: project\n---\n# Project X\nLink to [[Other Note]] with #tag1.";
        let note = parse_note(content);
        assert!(note.frontmatter.is_some());
        assert_eq!(note.title.as_deref(), Some("Project X"));
        assert_eq!(note.links.len(), 1);
        assert!(note.tags.contains(&"tag1".to_string()));
    }
}
