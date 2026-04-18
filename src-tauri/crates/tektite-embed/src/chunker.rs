//! Heading-based chunking with size guardrails and structural prefixes.
//!
//! The chunker performs three passes over a parsed note:
//! 1. **Heading split** — break the body on ATX headings (outside fenced
//!    code), tracking a heading-level stack to build hierarchical paths.
//! 2. **Guardrails** — sub-split chunks larger than [`MAX_TOKENS`] on
//!    paragraph boundaries, then merge chunks smaller than [`MIN_TOKENS`]
//!    into the following sibling whenever the merged result still fits.
//! 3. **Embed-input assembly** — prefix every chunk with
//!    `<title> / <heading-path>` so the embedder sees structural context.
//!    The SHA-256 hash is computed over the *prefixed* input, which means
//!    a title or heading-path change correctly invalidates the embedding.
//!
//! ## Token estimation
//!
//! The chunker is the wrong layer to depend on a real BERT tokenizer — it
//! must run quickly during scan_and_index and on every save. We use a
//! cheap approximation: ⌈chars / 4⌉, which matches BERT/WordPiece's
//! average compression on English prose closely enough to drive the
//! ±10% tolerance the guardrails care about. The exact token count used
//! by inference is computed by `OnnxEmbedder` independently.

use sha2::{Digest, Sha256};

use tektite_parser::ParsedNote;

/// Soft upper bound for chunk size (in approximate tokens). Chunks above
/// this are sub-split on paragraph boundaries.
pub const MAX_TOKENS: u32 = 512;

/// Soft lower bound for chunk size (in approximate tokens). Chunks below
/// this are merged into the following sibling when the merged total still
/// fits within `MAX_TOKENS`.
pub const MIN_TOKENS: u32 = 64;

/// A single chunk ready to be embedded and stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// 0-based ordinal within the source file.
    pub chunk_index: usize,
    /// Hierarchical heading path (e.g. `"Intro / Setup"`). `None` for chunks
    /// that appear before any heading in the file.
    pub heading_path: Option<String>,
    /// Leaf heading text (e.g. `"Setup"` from `"Intro / Setup"`). `None` for
    /// chunks that appear before any heading. Populated alongside
    /// [`heading_level`] so navigation handlers can scroll directly to the
    /// heading without re-parsing `heading_path`.
    pub heading_text: Option<String>,
    /// Markdown level of the leaf heading (1–6). `None` when `heading_text`
    /// is `None`.
    pub heading_level: Option<u8>,
    /// Raw chunk text — the body of the chunk without any added prefix.
    /// This is what gets stored in the `chunks.content` column and
    /// surfaced as the search-result snippet.
    pub content: String,
    /// What actually gets fed to the embedder: the chunk content prefixed
    /// with `<title> / <heading_path>` for structural context.
    pub embed_input: String,
    /// Hex SHA-256 of [`Chunk::embed_input`]. Used for change detection —
    /// the same prefixed input always hashes the same, so retitling a note
    /// correctly invalidates its chunks.
    pub content_hash: String,
    /// Approximate WordPiece token count for the prefixed embed input.
    /// Heuristic (`⌈chars / 4⌉`) — see module-level docs.
    pub token_count: u32,
}

/// Chunks a parsed note. `title` is woven into every chunk's
/// [`Chunk::embed_input`] so the embedder sees the note's name as part
/// of each section's context.
pub fn chunk_note(title: &str, note: &ParsedNote) -> Vec<Chunk> {
    let raw = split_by_headings(&note.body);
    apply_guardrails(raw, title)
}

// ---------------------------------------------------------------------------
// Pass 1: heading-based split
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct RawChunk {
    heading_path: Option<String>,
    heading_text: Option<String>,
    heading_level: Option<u8>,
    content: String,
}

fn split_by_headings(body: &str) -> Vec<RawChunk> {
    let mut stack: Vec<(u8, String)> = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_text: Option<String> = None;
    let mut current_level: Option<u8> = None;
    let mut buf = String::new();
    let mut in_fence = false;
    let mut chunks: Vec<RawChunk> = Vec::new();

    let push = |path: Option<String>,
                text: Option<String>,
                level: Option<u8>,
                buf: &mut String,
                out: &mut Vec<RawChunk>| {
        let trimmed = buf.trim();
        if !trimmed.is_empty() {
            out.push(RawChunk {
                heading_path: path,
                heading_text: text,
                heading_level: level,
                content: trimmed.to_string(),
            });
        }
        buf.clear();
    };

    for line in body.split_inclusive('\n') {
        let stripped = line.trim_end_matches(['\r', '\n']);
        let starts_fence = stripped.trim_start().starts_with("```")
            || stripped.trim_start().starts_with("~~~");
        if starts_fence {
            in_fence = !in_fence;
            buf.push_str(line);
            continue;
        }

        if !in_fence {
            if let Some((level, text)) = parse_atx_heading(stripped) {
                push(
                    current_path.clone(),
                    current_text.clone(),
                    current_level,
                    &mut buf,
                    &mut chunks,
                );
                while stack.last().is_some_and(|(l, _)| *l >= level) {
                    stack.pop();
                }
                stack.push((level, text.clone()));
                current_path = Some(join_path(&stack));
                current_text = Some(text);
                current_level = Some(level);
                buf.push_str(line);
                continue;
            }
        }

        buf.push_str(line);
    }

    push(current_path, current_text, current_level, &mut buf, &mut chunks);
    chunks
}

fn parse_atx_heading(line: &str) -> Option<(u8, String)> {
    let stripped = line.trim_start();
    let hash_count = stripped.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hash_count) {
        return None;
    }
    let rest = &stripped[hash_count..];
    let body = match rest.chars().next() {
        None => "",
        Some(' ' | '\t') => rest.trim_start(),
        _ => return None,
    };
    let text = body.trim_end().trim_end_matches('#').trim_end().to_string();
    Some((hash_count as u8, text))
}

fn join_path(stack: &[(u8, String)]) -> String {
    stack
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join(" / ")
}

// ---------------------------------------------------------------------------
// Pass 2: guardrails — sub-split oversized, merge undersized
// ---------------------------------------------------------------------------

fn apply_guardrails(raw: Vec<RawChunk>, title: &str) -> Vec<Chunk> {
    // 2a. Sub-split anything that's too big.
    let mut after_split: Vec<RawChunk> = Vec::with_capacity(raw.len());
    for chunk in raw {
        let est = estimate_tokens_with_prefix(title, chunk.heading_path.as_deref(), &chunk.content);
        if est > MAX_TOKENS {
            after_split.extend(sub_split(chunk, title));
        } else {
            after_split.push(chunk);
        }
    }

    // 2b. Merge tiny chunks into the next sibling. We do a single forward
    // pass — repeatedly applying merge wouldn't change behaviour because
    // the merged chunk inherits the next chunk's heading path and is
    // unlikely to fall back below the threshold.
    let mut after_merge: Vec<RawChunk> = Vec::with_capacity(after_split.len());
    let mut iter = after_split.into_iter().peekable();
    while let Some(cur) = iter.next() {
        let cur_tokens = estimate_tokens_with_prefix(title, cur.heading_path.as_deref(), &cur.content);
        if cur_tokens < MIN_TOKENS {
            if let Some(next) = iter.peek() {
                let next_tokens = estimate_tokens_with_prefix(
                    title,
                    next.heading_path.as_deref(),
                    &next.content,
                );
                if cur_tokens + next_tokens <= MAX_TOKENS {
                    let next = iter.next().expect("peeked next exists");
                    after_merge.push(merge_pair(&cur, &next));
                    continue;
                }
            }
        }
        after_merge.push(cur);
    }

    // 2c. Materialise final Chunks with assembled embed inputs and hashes.
    after_merge
        .into_iter()
        .enumerate()
        .map(|(idx, raw)| build_chunk(idx, title, raw))
        .collect()
}

fn sub_split(chunk: RawChunk, title: &str) -> Vec<RawChunk> {
    // Split on blank-line paragraph boundaries; greedy-pack paragraphs into
    // accumulators that stay under MAX_TOKENS. A single paragraph that
    // exceeds the budget on its own is left intact (Phase 2 does not
    // sentence-split).
    let paragraphs: Vec<&str> = chunk
        .content
        .split("\n\n")
        .map(|p| p.trim_matches('\n'))
        .filter(|p| !p.is_empty())
        .collect();

    if paragraphs.len() <= 1 {
        return vec![chunk];
    }

    let prefix_overhead = prefix_for(title, chunk.heading_path.as_deref()).chars().count();
    let mut out: Vec<RawChunk> = Vec::new();
    let mut acc = String::new();
    let mut acc_tokens = approx_tokens_from_chars(prefix_overhead);

    for para in paragraphs {
        let para_tokens = approx_tokens(para) + 1; // +1 for the joining blank line
        if !acc.is_empty() && acc_tokens + para_tokens > MAX_TOKENS {
            out.push(RawChunk {
                heading_path: chunk.heading_path.clone(),
                heading_text: chunk.heading_text.clone(),
                heading_level: chunk.heading_level,
                content: acc.trim().to_string(),
            });
            acc.clear();
            acc_tokens = approx_tokens_from_chars(prefix_overhead);
        }
        if !acc.is_empty() {
            acc.push_str("\n\n");
        }
        acc.push_str(para);
        acc_tokens += para_tokens;
    }
    if !acc.trim().is_empty() {
        out.push(RawChunk {
            heading_path: chunk.heading_path.clone(),
            heading_text: chunk.heading_text.clone(),
            heading_level: chunk.heading_level,
            content: acc.trim().to_string(),
        });
    }
    if out.is_empty() {
        out.push(chunk);
    }
    out
}

fn merge_pair(a: &RawChunk, b: &RawChunk) -> RawChunk {
    // The merged chunk takes the *next* sibling's heading path so the
    // dominant context is the section the merged content belongs to.
    let mut content = a.content.clone();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push('\n');
    content.push_str(&b.content);
    RawChunk {
        heading_path: b.heading_path.clone(),
        heading_text: b.heading_text.clone(),
        heading_level: b.heading_level,
        content,
    }
}

// ---------------------------------------------------------------------------
// Pass 3: embed-input assembly + hashing
// ---------------------------------------------------------------------------

fn build_chunk(index: usize, title: &str, raw: RawChunk) -> Chunk {
    let prefix = prefix_for(title, raw.heading_path.as_deref());
    let embed_input = format!("{prefix}\n\n{}", raw.content);
    let content_hash = sha256_hex(&embed_input);
    let token_count = approx_tokens(&embed_input);
    Chunk {
        chunk_index: index,
        heading_path: raw.heading_path,
        heading_text: raw.heading_text,
        heading_level: raw.heading_level,
        content: raw.content,
        embed_input,
        content_hash,
        token_count,
    }
}

fn prefix_for(title: &str, heading_path: Option<&str>) -> String {
    match heading_path {
        Some(path) if !path.is_empty() => format!("{title} / {path}"),
        _ => title.to_string(),
    }
}

fn estimate_tokens_with_prefix(title: &str, heading_path: Option<&str>, content: &str) -> u32 {
    let prefix = prefix_for(title, heading_path);
    approx_tokens_from_chars(prefix.chars().count() + 2 + content.chars().count())
}

fn approx_tokens(text: &str) -> u32 {
    approx_tokens_from_chars(text.chars().count())
}

fn approx_tokens_from_chars(chars: usize) -> u32 {
    // ⌈chars / 4⌉ — see module docs for the rationale.
    ((chars + 3) / 4) as u32
}

fn sha256_hex(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: &str) -> ParsedNote {
        tektite_parser::parse(body)
    }

    // ----- Heading split fundamentals (carried forward from Phase 1) -----

    #[test]
    fn empty_note_produces_no_chunks() {
        let chunks = chunk_note("Empty", &parse(""));
        assert!(chunks.is_empty());
    }

    #[test]
    fn note_with_no_headings_produces_a_single_chunk() {
        let note = parse("Just a paragraph of prose with no structure.\n");
        let chunks = chunk_note("Untitled", &note);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading_path, None);
        assert!(chunks[0].content.contains("Just a paragraph"));
    }

    #[test]
    fn single_heading_yields_one_chunk_with_path() {
        let chunks = chunk_note("T", &parse("# Only\nsome content here.\n"));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading_path.as_deref(), Some("Only"));
    }

    #[test]
    fn deeply_nested_headings_join_with_slashes() {
        let body = "# A\n## B\n### C\n#### D\n##### E\n###### F\nleaf\n";
        let chunks = chunk_note("Deep", &parse(body));
        // Each heading line on its own opens a new section. Since some
        // sections have only the heading line as content, they may or may
        // not survive — but the deepest section's path must be present.
        let deepest = chunks.last().unwrap();
        assert_eq!(deepest.heading_path.as_deref(), Some("A / B / C / D / E / F"));
    }

    #[test]
    fn fenced_code_block_hashes_are_not_treated_as_headings() {
        let body = "# Real\nprose\n```\n# not a heading\nmore\n```\ntail\n";
        let chunks = chunk_note("T", &parse(body));
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("# not a heading"));
    }

    // ----- Prefix injection -----

    #[test]
    fn embed_input_prefixes_title_and_heading_path() {
        let chunks = chunk_note("My Note", &parse("# Section\nbody text here.\n"));
        let chunk = &chunks[0];
        assert!(chunk.embed_input.starts_with("My Note / Section\n\n"));
        assert!(chunk.embed_input.contains("body text here"));
        // Stored content stays clean.
        assert!(!chunk.content.starts_with("My Note"));
    }

    #[test]
    fn embed_input_uses_title_only_when_no_heading_path() {
        let chunks = chunk_note("My Note", &parse("just a paragraph\n"));
        let chunk = &chunks[0];
        assert!(chunk.embed_input.starts_with("My Note\n\n"));
    }

    // ----- Hash stability + invalidation -----

    #[test]
    fn content_hash_is_stable_across_invocations() {
        let a = chunk_note("T", &parse("# A\nhello world example\n"));
        let b = chunk_note("T", &parse("# A\nhello world example\n"));
        assert_eq!(a[0].content_hash, b[0].content_hash);
    }

    #[test]
    fn hash_changes_when_title_changes() {
        let a = chunk_note("Title One", &parse("# A\nbody\n"));
        let b = chunk_note("Title Two", &parse("# A\nbody\n"));
        assert_ne!(a[0].content_hash, b[0].content_hash);
    }

    #[test]
    fn hash_changes_when_heading_path_changes() {
        let a = chunk_note("T", &parse("# A\nbody\n"));
        let b = chunk_note("T", &parse("# B\nbody\n"));
        assert_ne!(a[0].content_hash, b[0].content_hash);
    }

    // ----- Guardrails: sub-split -----

    #[test]
    fn oversized_chunks_subsplit_on_paragraph_boundaries() {
        // Build content of ~1000 paragraphs of 8 words each → ~8000 words.
        // Approx tokens ≈ chars/4 ≫ MAX_TOKENS, forcing sub-split.
        let para = "alpha beta gamma delta epsilon zeta eta theta";
        let body = (0..200)
            .map(|i| format!("Paragraph {i}: {para}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let body = format!("# Big\n{body}\n");
        let chunks = chunk_note("T", &parse(&body));
        assert!(
            chunks.len() > 1,
            "expected multiple sub-chunks, got {}",
            chunks.len()
        );
        for c in &chunks {
            assert!(
                c.token_count <= MAX_TOKENS + 8, // small slack for prefix rounding
                "chunk {} exceeded MAX_TOKENS: {}",
                c.chunk_index,
                c.token_count
            );
            assert_eq!(c.heading_path.as_deref(), Some("Big"));
        }
    }

    #[test]
    fn single_paragraph_too_large_is_left_intact() {
        // No paragraph boundaries to split on → one chunk, oversized.
        let blob = "word ".repeat(2000);
        let body = format!("# Solo\n{blob}\n");
        let chunks = chunk_note("T", &parse(&body));
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].token_count > MAX_TOKENS);
    }

    // ----- Guardrails: merge -----

    #[test]
    fn small_chunks_merge_into_next_sibling() {
        // Two tiny adjacent sections — both well under MIN_TOKENS, easily
        // fit together under MAX_TOKENS → one merged chunk.
        let body = "# A\nshort.\n# B\nalso short.\n";
        let chunks = chunk_note("T", &parse(&body));
        assert_eq!(chunks.len(), 1, "two tiny chunks should collapse to one");
        assert_eq!(chunks[0].heading_path.as_deref(), Some("B"));
        assert!(chunks[0].content.contains("short"));
        assert!(chunks[0].content.contains("also short"));
    }

    #[test]
    fn merge_does_not_happen_when_combined_size_exceeds_max() {
        // Tiny first chunk + huge second chunk → don't merge; emit as-is.
        let big = "word ".repeat(2500); // > MAX_TOKENS in a single paragraph
        let body = format!("# A\nshort.\n# B\n{big}\n");
        let chunks = chunk_note("T", &parse(&body));
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].heading_path.as_deref(), Some("A"));
        assert_eq!(chunks[1].heading_path.as_deref(), Some("B"));
    }

    #[test]
    fn merge_at_end_of_document_leaves_trailing_small_chunk_alone() {
        // Last section is tiny but has no successor → emitted as-is.
        let body = "# A\n".to_string() + &"text ".repeat(100) + "\n# B\nshort.\n";
        let chunks = chunk_note("T", &parse(&body));
        assert!(chunks.last().unwrap().heading_path.as_deref() == Some("B"));
    }

    // ----- Token count is stored -----

    #[test]
    fn token_count_is_populated() {
        let chunks = chunk_note("T", &parse("# A\nhello world\n"));
        assert!(chunks[0].token_count > 0);
    }

    // ----- heading_text / heading_level -----

    #[test]
    fn heading_text_and_level_match_leaf_of_path() {
        let body = "# A\n## B\n### C\nleaf body text here with content\n";
        let chunks = chunk_note("T", &parse(body));
        let last = chunks.last().unwrap();
        assert_eq!(last.heading_path.as_deref(), Some("A / B / C"));
        assert_eq!(last.heading_text.as_deref(), Some("C"));
        assert_eq!(last.heading_level, Some(3));
    }

    #[test]
    fn heading_text_is_none_for_headingless_chunk() {
        let chunks = chunk_note("T", &parse("just prose with no headings at all\n"));
        assert_eq!(chunks[0].heading_path, None);
        assert_eq!(chunks[0].heading_text, None);
        assert_eq!(chunks[0].heading_level, None);
    }

    #[test]
    fn merged_chunk_inherits_next_siblings_heading_text() {
        let body = "# A\nshort.\n# B\nalso short.\n";
        let chunks = chunk_note("T", &parse(body));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading_text.as_deref(), Some("B"));
        assert_eq!(chunks[0].heading_level, Some(1));
    }
}
