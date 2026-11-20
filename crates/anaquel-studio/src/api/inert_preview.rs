//! The HTML of a preview without its `<script>` elements.
//!
//! The editor shows the preview in a frame whose `sandbox` has no `allow-scripts`, so no script of
//! the page can run there. A browser still reports every script it refuses, once per element, in
//! the developer console, and those reports hide the messages that matter. The scripts of the
//! theme (`init`, `main`) are therefore taken out of the HTML before it reaches the frame; the
//! build itself, and the byte-for-byte match between a preview and a built page, are not touched,
//! because the removal happens only in the body of the preview request.
//!
//! The removal is not a defence: the sandbox and the policy of the page stay the only barrier. It
//! relies on the fact that the text of a page cannot put a `<script` into the HTML (the composer
//! escapes it), so every element found here comes from the theme and has no markup inside.

/// Returns `html` with every `<script ...>...</script>` element removed.
///
/// A match needs `<script` followed by whitespace, `>` or `/` (so `<scripted>` stays), and a
/// closing `</script` followed by the same and then a `>` (so `</scripted>` does not close it).
/// A `<script` inside an HTML comment is text and stays. An element that is never closed is left
/// as it is, with everything after it: the frame then blocks it as before, and nothing is cut by
/// guesswork. Letter case is ignored in the tag names.
pub(crate) fn without_scripts(html: &str) -> String {
    // Lowercasing ASCII keeps every byte offset, so positions found here are valid in `html`.
    let lower = html.to_ascii_lowercase();
    let mut kept = String::with_capacity(html.len());
    let mut cursor = 0;
    while let Some(start) = next_opening(&lower, cursor) {
        let Some(end) = closing_end(&lower, start) else {
            break;
        };
        kept.push_str(&html[cursor..start]);
        cursor = end;
    }
    kept.push_str(&html[cursor..]);
    kept
}

/// Whether the tag name that ends at `after` is complete: the next character is whitespace, `/` or
/// `>`, as the HTML parser requires. `<scripted>` and `</scripts>` are other tags.
fn name_ends_at(lower: &str, after: usize) -> bool {
    lower[after..]
        .chars()
        .next()
        .is_some_and(|next| next == '>' || next == '/' || next.is_ascii_whitespace())
}

/// The offset of the next `<script` tag at or after `from`, skipping look-alikes such as
/// `<scripted>` and everything inside an HTML comment, where a `<script` is only text. A comment
/// that is never closed runs to the end of the document, as it does for the browser.
fn next_opening(lower: &str, from: usize) -> Option<usize> {
    const OPENING: &str = "<script";
    const COMMENT_OPENING: &str = "<!--";
    const COMMENT_CLOSING: &str = "-->";
    let mut search = from;
    loop {
        let rest = &lower[search..];
        let script = rest.find(OPENING)?;
        if let Some(comment) = rest
            .find(COMMENT_OPENING)
            .filter(|comment| *comment < script)
        {
            // The parser lets `<!-->` and `<!--->` close at once, so the search for the end of
            // the comment starts after `<!`, not after `<!--`.
            let from_dashes = search + comment + "<!".len();
            search =
                from_dashes + lower[from_dashes..].find(COMMENT_CLOSING)? + COMMENT_CLOSING.len();
            continue;
        }
        let start = search + script;
        let after = start + OPENING.len();
        if name_ends_at(lower, after) {
            return Some(start);
        }
        search = after;
    }
}

/// The offset just past the `>` of the `</script ...>` that closes the element opened at `start`.
fn closing_end(lower: &str, start: usize) -> Option<usize> {
    const CLOSING: &str = "</script";
    let mut search = start;
    loop {
        let close = search + lower[search..].find(CLOSING)?;
        let after = close + CLOSING.len();
        if name_ends_at(lower, after) {
            let tag_end = after + lower[after..].find('>')?;
            return Some(tag_end + 1);
        }
        search = after;
    }
}

#[cfg(test)]
mod tests;
