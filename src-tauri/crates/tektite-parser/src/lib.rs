//! `tektite-parser` — Pure markdown metadata extraction.
//!
//! Extracts frontmatter, wiki-links, tags, headings, and tasks from markdown
//! files. All extraction is AST-aware: wiki-links and tags are never extracted
//! from fenced code blocks or inline code spans.

use std::collections::HashMap;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("YAML deserialization failed: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A wiki-link extracted from a markdown document.
///
/// Represents links of the forms:
/// - `[[note]]`              → `target="note"`, `fragment=None`, `alias=None`
/// - `[[note#heading]]`      → `target="note"`, `fragment=Some("heading")`, `alias=None`
/// - `[[note|display]]`      → `target="note"`, `fragment=None`, `alias=Some("display")`
/// - `[[note#heading|disp]]` → all three fields populated
/// - `[[folder/note]]`       → path-qualified target
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedLink {
    /// The note target (filename stem or path-qualified target).
    pub target: String,
    /// Optional heading or block-reference fragment after `#`.
    pub fragment: Option<String>,
    /// Optional display text after `|`.
    pub alias: Option<String>,
}

/// A heading extracted from a markdown document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedHeading {
    /// ATX heading level: 1 through 6.
    pub level: u8,
    /// Plain-text content of the heading (inline markup stripped).
    pub text: String,
}

/// A task item (checkbox list item) extracted from a markdown document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedTask {
    /// Plain-text description of the task.
    pub text: String,
    /// `true` if the checkbox is checked (`[x]` or `[X]`).
    pub done: bool,
}

/// All metadata extracted from a single markdown file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedNote {
    /// Parsed frontmatter key-value pairs. Empty map if no frontmatter.
    pub frontmatter: HashMap<String, serde_yaml::Value>,
    /// Raw body text with frontmatter stripped (if any).
    pub body: String,
    /// Wiki-links found in body text outside code contexts.
    pub links: Vec<ParsedLink>,
    /// Hashtags found in body text outside code contexts (without leading `#`).
    pub tags: Vec<String>,
    /// Headings in document order.
    pub headings: Vec<ParsedHeading>,
    /// Task items in document order.
    pub tasks: Vec<ParsedTask>,
}

// ---------------------------------------------------------------------------
// Compiled regexes (initialized once)
// ---------------------------------------------------------------------------

fn wiki_link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Captures:
    //   group 1 — target           (required, no ], #, |)
    //   group 2 — fragment after # (optional, no ], |)
    //   group 3 — alias after |    (optional, no ])
    RE.get_or_init(|| Regex::new(r"\[\[([^\]#|]+?)(?:#([^\]|]+?))?(?:\|([^\]]+?))?\]\]").unwrap())
}

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches #tag where tag starts with a letter/digit and contains word chars, slashes, or hyphens.
    // We capture an optional leading whitespace/start boundary without lookbehind by allowing
    // either start-of-string or a whitespace character before the `#`, then we only keep
    // the tag name in capture group 1.
    // Pattern: (start | whitespace) followed by # followed by tag chars.
    RE.get_or_init(|| Regex::new(r"(?:^|\s)#([a-zA-Z0-9][a-zA-Z0-9/_-]*)").unwrap())
}

// ---------------------------------------------------------------------------
// Frontmatter extraction (A2: line-by-line, CRLF-safe, whitespace-tolerant)
// ---------------------------------------------------------------------------

/// Splits `content` into `(frontmatter_yaml, body)`.
///
/// Frontmatter is bounded by `---` fence lines. A fence line is any line
/// whose content, after stripping a trailing `\r`, matches `^---\s*$`.
///
/// Returns `(None, content)` when no valid frontmatter block is found.
fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    // Iterate over lines, preserving byte offsets so we can slice the original.
    let mut lines = content.split('\n').peekable();

    // Opening fence must be the very first line.
    let first_line = match lines.next() {
        Some(l) => l,
        None => return (None, content),
    };

    if !is_fence_line(first_line) {
        return (None, content);
    }

    // Byte offset just after the newline following the opening fence.
    let after_open_fence = first_line.len() + 1; // +1 for the '\n'

    // Scan for the closing fence.
    let mut offset = after_open_fence;
    for line in lines {
        if is_fence_line(line) {
            // `offset` is the start of this closing fence line.
            let yaml_src = &content[after_open_fence..offset];
            // Body is everything after the closing fence line + its newline.
            let body_start = offset + line.len() + 1; // +1 for '\n'
            let body = if body_start < content.len() {
                &content[body_start..]
            } else {
                ""
            };
            return (Some(yaml_src), body);
        }
        offset += line.len() + 1; // +1 for '\n'
    }

    // No closing fence found.
    (None, content)
}

/// Returns `true` if `line` (which may have a trailing `\r`) matches `^---\s*$`.
#[inline]
fn is_fence_line(line: &str) -> bool {
    // Strip optional trailing carriage return for CRLF files.
    let line = line.strip_suffix('\r').unwrap_or(line);
    line == "---" || (line.starts_with("---") && line[3..].chars().all(char::is_whitespace))
}

/// Parses YAML frontmatter into a key-value map.
///
/// Returns an empty map on parse failure rather than propagating errors,
/// since malformed frontmatter should not prevent body parsing.
fn parse_frontmatter_yaml(yaml: &str) -> HashMap<String, serde_yaml::Value> {
    serde_yaml::from_str(yaml).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// AST-aware extraction (A1: wiki-links and tags outside code contexts)
// ---------------------------------------------------------------------------

/// Extracts wiki-links, tags, headings, and tasks from `body` using a single
/// pulldown-cmark event walk.
///
/// Wiki-links and tags are extracted only from text that is **not** inside a
/// fenced code block or inline code span.
///
/// ## Why we buffer text
///
/// pulldown-cmark splits `[[note]]` into multiple consecutive `Event::Text`
/// tokens: `"["`, `"["`, `"note"`, `"]"`, `"]"`. Applying a wiki-link regex to
/// individual tokens would never see the full `[[...]]` pattern.
///
/// We solve this by accumulating consecutive text tokens into a per-block
/// buffer and flushing it when a structural boundary is crossed (heading end,
/// paragraph end, list item end, code block start, etc.).
fn extract_from_body(
    body: &str,
) -> (
    Vec<ParsedLink>,
    Vec<String>,
    Vec<ParsedHeading>,
    Vec<ParsedTask>,
) {
    let mut links: Vec<ParsedLink> = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    let mut headings: Vec<ParsedHeading> = Vec::new();
    let mut tasks: Vec<ParsedTask> = Vec::new();

    let options =
        Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;

    let parser = Parser::new_ext(body, options);

    // State tracking.
    let mut in_code_block = false;
    let mut in_heading: Option<u8> = None;

    // Accumulated plain-text for the current non-code block.
    // Flushed at structural boundaries to run the wiki-link / tag regexes.
    let mut prose_buf = String::new();

    // Heading text accumulator (separate because we need it for the heading record).
    let mut heading_text_buf = String::new();

    // Task state.
    let mut in_task_item = false;
    let mut task_done = false;
    let mut task_text_buf = String::new();

    /// Flush `prose_buf` into `links` and `tags`, then clear it.
    macro_rules! flush_prose {
        () => {
            if !prose_buf.is_empty() {
                extract_wiki_links_from_text(&prose_buf, &mut links);
                extract_tags_from_text(&prose_buf, &mut tags);
                prose_buf.clear();
            }
        };
    }

    for event in parser {
        match event {
            // -- Code block entry/exit --
            Event::Start(Tag::CodeBlock(_)) => {
                flush_prose!();
                in_code_block = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                // Content inside the code block was not appended to prose_buf,
                // so nothing to flush here.
                in_code_block = false;
            }

            // Inline code is a single opaque event — skip its content.
            Event::Code(_) => {
                // Flush prose accumulated so far so that regex can't straddle
                // across inline code (e.g., `[[foo` + `bar]]` must not match).
                flush_prose!();
            }

            // -- Heading entry/exit --
            Event::Start(Tag::Heading { level, .. }) => {
                flush_prose!();
                in_heading = Some(level as u8);
                heading_text_buf.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                flush_prose!();
                if let Some(level) = in_heading.take() {
                    headings.push(ParsedHeading {
                        level,
                        text: heading_text_buf.trim().to_string(),
                    });
                }
                heading_text_buf.clear();
            }

            // Flush at paragraph / block boundaries so the regex window is
            // never larger than one block.
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {
                flush_prose!();
            }
            Event::Start(Tag::BlockQuote(_)) | Event::End(TagEnd::BlockQuote(_)) => {
                flush_prose!();
            }

            // -- Task list items --
            Event::Start(Tag::Item) => {
                flush_prose!();
                in_task_item = false;
                task_done = false;
                task_text_buf.clear();
            }
            Event::TaskListMarker(checked) => {
                in_task_item = true;
                task_done = checked;
            }
            Event::End(TagEnd::Item) => {
                flush_prose!();
                if in_task_item {
                    tasks.push(ParsedTask {
                        text: task_text_buf.trim().to_string(),
                        done: task_done,
                    });
                    in_task_item = false;
                }
                task_text_buf.clear();
            }

            // -- Text nodes: accumulate into buffers --
            Event::Text(text) => {
                // Always accumulate heading text (for the heading record).
                if in_heading.is_some() {
                    heading_text_buf.push_str(&text);
                }

                // Always accumulate task text.
                if in_task_item {
                    task_text_buf.push_str(&text);
                }

                // Accumulate for wiki-link / tag extraction only outside code.
                if !in_code_block {
                    prose_buf.push_str(&text);
                }
            }

            // SoftBreak / HardBreak within a block.
            Event::SoftBreak | Event::HardBreak => {
                if in_heading.is_some() {
                    heading_text_buf.push(' ');
                }
                if in_task_item {
                    task_text_buf.push(' ');
                }
                if !in_code_block {
                    prose_buf.push(' ');
                }
            }

            _ => {}
        }
    }

    // Final flush in case the document ends without a closing boundary.
    if !prose_buf.is_empty() {
        extract_wiki_links_from_text(&prose_buf, &mut links);
        extract_tags_from_text(&prose_buf, &mut tags);
    }

    (links, tags, headings, tasks)
}

/// Applies the wiki-link regex to a single text buffer and appends results.
fn extract_wiki_links_from_text(text: &str, out: &mut Vec<ParsedLink>) {
    for cap in wiki_link_re().captures_iter(text) {
        let target = cap[1].trim().to_string();
        let fragment = cap.get(2).map(|m| m.as_str().to_string());
        let alias = cap.get(3).map(|m| m.as_str().to_string());
        out.push(ParsedLink {
            target,
            fragment,
            alias,
        });
    }
}

/// Applies the tag regex to a single text node and appends bare tag names.
fn extract_tags_from_text(text: &str, out: &mut Vec<String>) {
    for cap in tag_re().captures_iter(text) {
        out.push(cap[1].to_string());
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parses a full markdown file and returns all extracted metadata.
///
/// # Errors
///
/// Currently infallible — parse errors in frontmatter YAML result in an empty
/// frontmatter map rather than an error. This may change in a future version.
///
/// # Example
///
/// ```
/// let note = tektite_parser::parse("---\ntitle: My Note\n---\n# Hello\n[[other]]\n");
/// assert_eq!(note.headings[0].text, "Hello");
/// assert_eq!(note.links[0].target, "other");
/// ```
pub fn parse(content: &str) -> ParsedNote {
    let (yaml_src, body) = split_frontmatter(content);

    let frontmatter = yaml_src.map(parse_frontmatter_yaml).unwrap_or_default();

    let (links, tags, headings, tasks) = extract_from_body(body);

    ParsedNote {
        frontmatter,
        body: body.to_string(),
        links,
        tags,
        headings,
        tasks,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn links(src: &str) -> Vec<ParsedLink> {
        parse(src).links
    }

    fn tags(src: &str) -> Vec<String> {
        parse(src).tags
    }

    fn link(target: &str, fragment: Option<&str>, alias: Option<&str>) -> ParsedLink {
        ParsedLink {
            target: target.to_string(),
            fragment: fragment.map(str::to_string),
            alias: alias.map(str::to_string),
        }
    }

    // -----------------------------------------------------------------------
    // A1: AST-aware extraction — code block exclusion
    // -----------------------------------------------------------------------

    #[test]
    fn wiki_link_in_fenced_code_block_is_not_extracted() {
        let src = "```\n[[note]]\n```\n";
        assert!(links(src).is_empty());
    }

    #[test]
    fn wiki_link_in_indented_code_block_is_not_extracted() {
        // pulldown-cmark treats 4-space-indented blocks as code.
        let src = "    [[note]]\n";
        assert!(links(src).is_empty());
    }

    #[test]
    fn wiki_link_in_inline_code_is_not_extracted() {
        let src = "Text with `[[note]]` inline code.\n";
        assert!(links(src).is_empty());
    }

    #[test]
    fn tag_in_fenced_code_block_is_not_extracted() {
        let src = "```\n#mytag\n```\n";
        assert!(tags(src).is_empty());
    }

    #[test]
    fn tag_in_inline_code_is_not_extracted() {
        let src = "Use `#tag` for tagging.\n";
        assert!(tags(src).is_empty());
    }

    #[test]
    fn wiki_link_in_normal_text_is_extracted() {
        let src = "See [[my-note]] for details.\n";
        assert_eq!(links(src), vec![link("my-note", None, None)]);
    }

    #[test]
    fn tag_in_normal_text_is_extracted() {
        let src = "This is #important work.\n";
        assert_eq!(tags(src), vec!["important".to_string()]);
    }

    #[test]
    fn wiki_link_in_heading_is_extracted() {
        let src = "## See [[related-note]]\n";
        assert_eq!(links(src), vec![link("related-note", None, None)]);
    }

    #[test]
    fn tag_in_heading_is_extracted() {
        let src = "## Meeting #project-alpha\n";
        assert_eq!(tags(src), vec!["project-alpha".to_string()]);
    }

    #[test]
    fn wiki_link_in_blockquote_is_extracted() {
        let src = "> See [[note-in-quote]]\n";
        assert_eq!(links(src), vec![link("note-in-quote", None, None)]);
    }

    #[test]
    fn wiki_link_in_list_item_is_extracted() {
        let src = "- [[list-note]]\n";
        assert_eq!(links(src), vec![link("list-note", None, None)]);
    }

    #[test]
    fn multiple_links_and_tags_outside_code_extracted() {
        let src = "[[a]] #tag1 and [[b]] #tag2\n```\n[[c]] #tag3\n```\n";
        let note = parse(src);
        assert_eq!(
            note.links,
            vec![link("a", None, None), link("b", None, None)]
        );
        assert_eq!(note.tags, vec!["tag1".to_string(), "tag2".to_string()]);
    }

    // -----------------------------------------------------------------------
    // A2: Robust frontmatter fence detection
    // -----------------------------------------------------------------------

    #[test]
    fn frontmatter_lf_parses_correctly() {
        let src = "---\ntitle: Hello\n---\nBody text\n";
        let note = parse(src);
        assert_eq!(
            note.frontmatter.get("title").and_then(|v| v.as_str()),
            Some("Hello")
        );
        assert_eq!(note.body.trim(), "Body text");
    }

    #[test]
    fn frontmatter_crlf_parses_correctly() {
        let src = "---\r\ntitle: Hello\r\n---\r\nBody text\r\n";
        let note = parse(src);
        assert_eq!(
            note.frontmatter.get("title").and_then(|v| v.as_str()),
            Some("Hello")
        );
    }

    #[test]
    fn frontmatter_closing_fence_with_trailing_whitespace_is_recognized() {
        let src = "---\ntitle: Test\n---   \nBody\n";
        let note = parse(src);
        assert_eq!(
            note.frontmatter.get("title").and_then(|v| v.as_str()),
            Some("Test")
        );
        assert!(note.body.contains("Body"));
    }

    #[test]
    fn frontmatter_closing_fence_with_trailing_non_whitespace_is_not_a_fence() {
        // "---x" is not a valid fence; frontmatter goes unclosed → empty map.
        let src = "---\ntitle: Test\n---x\nBody\n";
        let note = parse(src);
        assert!(note.frontmatter.is_empty());
    }

    #[test]
    fn missing_closing_fence_returns_empty_frontmatter() {
        let src = "---\ntitle: No closing fence\nBody\n";
        let note = parse(src);
        assert!(note.frontmatter.is_empty());
    }

    #[test]
    fn no_frontmatter_returns_empty_map() {
        let src = "# Just a heading\n";
        let note = parse(src);
        assert!(note.frontmatter.is_empty());
    }

    #[test]
    fn body_text_after_frontmatter_is_correctly_separated() {
        let src = "---\ntitle: A\n---\n# My Heading\n\nParagraph.\n";
        let note = parse(src);
        assert!(!note.body.contains("title: A"));
        assert!(note.body.contains("My Heading"));
        assert!(note.body.contains("Paragraph"));
    }

    #[test]
    fn frontmatter_aliases_array_is_parsed() {
        let src = "---\naliases:\n  - Alias One\n  - Alias Two\n---\n";
        let note = parse(src);
        let aliases = note
            .frontmatter
            .get("aliases")
            .and_then(|v| v.as_sequence());
        assert!(aliases.is_some());
        assert_eq!(aliases.unwrap().len(), 2);
    }

    // -----------------------------------------------------------------------
    // A3: [[note#heading]] fragment parsing
    // -----------------------------------------------------------------------

    #[test]
    fn plain_wiki_link_has_no_fragment() {
        let src = "[[note]]\n";
        assert_eq!(links(src), vec![link("note", None, None)]);
    }

    #[test]
    fn wiki_link_with_heading_fragment_is_parsed() {
        let src = "[[note#Introduction]]\n";
        assert_eq!(links(src), vec![link("note", Some("Introduction"), None)]);
    }

    #[test]
    fn wiki_link_fragment_preserves_spaces() {
        let src = "[[note#Some Heading Text]]\n";
        assert_eq!(
            links(src),
            vec![link("note", Some("Some Heading Text"), None)]
        );
    }

    #[test]
    fn wiki_link_fragment_and_alias_are_both_parsed() {
        let src = "[[note#heading|display text]]\n";
        assert_eq!(
            links(src),
            vec![link("note", Some("heading"), Some("display text"))]
        );
    }

    #[test]
    fn wiki_link_alias_without_fragment_still_works() {
        let src = "[[note|display]]\n";
        assert_eq!(links(src), vec![link("note", None, Some("display"))]);
    }

    #[test]
    fn wiki_link_path_qualified_with_fragment() {
        let src = "[[folder/note#heading]]\n";
        assert_eq!(links(src), vec![link("folder/note", Some("heading"), None)]);
    }

    // -----------------------------------------------------------------------
    // Headings
    // -----------------------------------------------------------------------

    #[test]
    fn headings_are_extracted_at_correct_levels() {
        let src = "# H1\n## H2\n### H3\n";
        let note = parse(src);
        assert_eq!(note.headings.len(), 3);
        assert_eq!(
            note.headings[0],
            ParsedHeading {
                level: 1,
                text: "H1".into()
            }
        );
        assert_eq!(
            note.headings[1],
            ParsedHeading {
                level: 2,
                text: "H2".into()
            }
        );
        assert_eq!(
            note.headings[2],
            ParsedHeading {
                level: 3,
                text: "H3".into()
            }
        );
    }

    #[test]
    fn heading_plain_text_is_extracted_from_heading_with_inline_markup() {
        let src = "## **Bold** and *italic*\n";
        let note = parse(src);
        assert_eq!(note.headings[0].text, "Bold and italic");
    }

    // -----------------------------------------------------------------------
    // Tasks
    // -----------------------------------------------------------------------

    #[test]
    fn unchecked_task_is_extracted() {
        let src = "- [ ] Do the thing\n";
        let note = parse(src);
        assert_eq!(note.tasks.len(), 1);
        assert_eq!(note.tasks[0].text, "Do the thing");
        assert!(!note.tasks[0].done);
    }

    #[test]
    fn checked_task_is_extracted_as_done() {
        let src = "- [x] Done already\n";
        let note = parse(src);
        assert!(note.tasks[0].done);
    }

    // -----------------------------------------------------------------------
    // Round-trip smoke test
    // -----------------------------------------------------------------------

    #[test]
    fn full_parse_round_trip() {
        let src = concat!(
            "---\ntitle: Full Test\naliases:\n  - Test Note\n---\n",
            "# Introduction\n\n",
            "See [[other-note#section|other]] and #research.\n\n",
            "```rust\n[[not-a-link]] #not-a-tag\n```\n\n",
            "- [ ] Unchecked task\n",
            "- [x] Checked task\n",
        );
        let note = parse(src);

        assert_eq!(
            note.frontmatter.get("title").and_then(|v| v.as_str()),
            Some("Full Test")
        );
        assert_eq!(note.headings[0].text, "Introduction");
        assert_eq!(
            note.links,
            vec![link("other-note", Some("section"), Some("other"))]
        );
        assert_eq!(note.tags, vec!["research".to_string()]);
        assert_eq!(note.tasks.len(), 2);
        assert!(!note.tasks[0].done);
        assert!(note.tasks[1].done);
    }
}
