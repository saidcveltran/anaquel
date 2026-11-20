use super::without_scripts;

/// The head of a built page: two scripts of the theme, one with `defer`, one without.
const THEME_HEAD: &str = concat!(
    "<link rel=\"stylesheet\" href=\"/assets/styles/main.1bcf986720.css\">\n",
    "<script src=\"/assets/scripts/init.dc838c2b11.js\"></script>\n",
    "<script src=\"/assets/scripts/main.2281a02c84.js\" defer></script>\n",
);

#[test]
fn the_scripts_of_the_theme_are_removed_and_the_rest_of_the_head_stays() {
    let cleaned = without_scripts(THEME_HEAD);
    assert_eq!(
        cleaned,
        "<link rel=\"stylesheet\" href=\"/assets/styles/main.1bcf986720.css\">\n\n\n"
    );
}

#[test]
fn a_page_without_scripts_comes_back_identical() {
    let page = "<!doctype html><html><body><p>a &lt;script&gt; b</p></body></html>";
    assert_eq!(without_scripts(page), page);
    assert_eq!(without_scripts(""), "");
}

#[test]
fn a_script_with_a_body_is_removed_whole_even_when_the_body_has_angle_brackets() {
    let page = "<p>a</p><script>if (1 < 2) { x = \"<b>\"; }</script><p>b</p>";
    assert_eq!(without_scripts(page), "<p>a</p><p>b</p>");
}

#[test]
fn the_tag_names_are_matched_without_regard_to_letter_case() {
    let page = "<p>a</p><SCRIPT SRC=\"/x.js\"></ScRiPt ><p>b</p>";
    assert_eq!(without_scripts(page), "<p>a</p><p>b</p>");
}

#[test]
fn a_slash_or_a_newline_after_the_name_still_opens_a_script() {
    // The HTML parser starts a script at `<script/` and at `<script` plus a line break.
    assert_eq!(
        without_scripts("<p>a</p><script/src=\"/x.js\"></script><p>b</p>"),
        "<p>a</p><p>b</p>"
    );
    assert_eq!(
        without_scripts("<p>a</p><script\nsrc=\"/x.js\"\n></script><p>b</p>"),
        "<p>a</p><p>b</p>"
    );
}

#[test]
fn a_closing_tag_with_spaces_before_its_end_is_still_the_end() {
    let page = "<p>a</p><script>x</script   ><p>b</p>";
    assert_eq!(without_scripts(page), "<p>a</p><p>b</p>");
}

#[test]
fn a_tag_that_only_starts_like_script_is_kept() {
    for page in [
        "<scripted>x</scripted>",
        "<script-box>x</script-box>",
        "<scripts>x</scripts>",
        "<p>text</p><scriptx src=\"/a.js\"></scriptx>",
    ] {
        assert_eq!(without_scripts(page), page);
    }
}

#[test]
fn the_text_of_a_page_that_names_a_script_is_not_markup_and_stays() {
    // The composer writes a `<` of the text as `&lt;`, so this is what a page about scripts holds.
    let page = "<p>Use &lt;script src=\"a.js\"&gt;&lt;/script&gt; to load it.</p><pre><code>&lt;script&gt;x&lt;/script&gt;</code></pre>";
    assert_eq!(without_scripts(page), page);
}

#[test]
fn a_script_inside_a_comment_is_text_and_the_comment_stays_whole() {
    let page =
        "<!-- <script src=\"/x.js\"></script> --><p>a</p><script src=\"/y.js\"></script><p>b</p>";
    assert_eq!(
        without_scripts(page),
        "<!-- <script src=\"/x.js\"></script> --><p>a</p><p>b</p>"
    );
    // A comment that is never closed holds everything after it, as it does for the browser.
    let open = "<p>a</p><!-- <script></script><p>b</p>";
    assert_eq!(without_scripts(open), open);
}

#[test]
fn a_comment_after_other_content_ends_where_its_own_closing_is() {
    // The script that follows the comment is a real one, whatever comes before the comment.
    let page = "<p>a</p><!-- c --><script src=\"/x.js\"></script><p>b</p>";
    assert_eq!(without_scripts(page), "<p>a</p><!-- c --><p>b</p>");
    // A `-->` that is plain text before the comment does not end the comment that follows it.
    let text_first = "a --> b <!-- <script src=\"/x.js\"></script> --><p>k</p>";
    assert_eq!(without_scripts(text_first), text_first);
}

#[test]
fn a_comment_after_a_removed_script_is_found_from_where_the_search_resumes() {
    // The second search starts after the first script, not at the start of the document.
    let page =
        "<script src=\"/a.js\"></script>x --> <!-- <script src=\"/b.js\"></script> --><p>k</p>";
    assert_eq!(
        without_scripts(page),
        "x --> <!-- <script src=\"/b.js\"></script> --><p>k</p>"
    );
}

#[test]
fn the_shortest_comments_end_at_once() {
    // `<!-->` and `<!--->` are complete comments for the HTML parser.
    for comment in ["<!-->", "<!--->"] {
        let page = format!("{comment}<script src=\"/x.js\"></script><p>b</p>");
        assert_eq!(without_scripts(&page), format!("{comment}<p>b</p>"));
    }
}

#[test]
fn a_closing_tag_that_only_starts_like_script_does_not_close_it() {
    let page = "<script>a</scripted>b</script><p>k</p>";
    assert_eq!(without_scripts(page), "<p>k</p>");
    // With no real closing tag after the look-alike, the element is never closed.
    let open = "<p>a</p><script>a</scripts>b";
    assert_eq!(without_scripts(open), open);
}

#[test]
fn an_element_that_is_never_closed_is_left_as_it_is() {
    let page = "<p>a</p><script src=\"/x.js\"><p>b</p>";
    assert_eq!(without_scripts(page), page);
    // A closing tag without its `>` is not a closing tag either.
    let cut = "<p>a</p><script>x</script";
    assert_eq!(without_scripts(cut), cut);
}

#[test]
fn only_the_closed_elements_are_removed_when_a_later_one_is_cut_short() {
    let page = "<a></a><script>1</script><b></b><script>2";
    assert_eq!(without_scripts(page), "<a></a><b></b><script>2");
}

#[test]
fn non_ascii_text_around_the_scripts_is_kept_byte_for_byte() {
    let page = "<p>año — 日本語 ñ</p><script src=\"/x.js\"></script><p>ü 🙂</p>";
    assert_eq!(without_scripts(page), "<p>año — 日本語 ñ</p><p>ü 🙂</p>");
}

#[test]
fn removing_twice_gives_the_same_as_removing_once() {
    let once = without_scripts(THEME_HEAD);
    assert_eq!(without_scripts(&once), once);
}
