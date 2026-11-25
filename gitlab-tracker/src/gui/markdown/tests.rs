use super::*;

fn gitlab(source: &str) -> String {
    let links = Links::new("https://git.io/org/app/-/merge_requests/7", "main");
    prepare(source, &links)
}

#[test]
fn references_become_links() {
    assert_eq!(
        gitlab("See !12, #34 and @jo_ann."),
        "See [!12](<https://git.io/org/app/-/merge_requests/12>), \
         [#34](<https://git.io/org/app/-/issues/34>) and \
         [@jo\\_ann](<https://git.io/jo_ann>)."
    );
}

#[test]
fn what_only_looks_like_a_reference_stays() {
    for text in [
        "a@b.io", "C#1", "hello!12", "#12a", "&#45;", "\\#12", "@all", "# 12",
    ] {
        assert_eq!(gitlab(text), text);
    }
}

#[test]
fn code_stays_as_written() {
    for text in [
        "`#12 <b>`",
        "```\n#12 <!-- x --> https://a.io\n```",
        "    !12 <br>",
    ] {
        assert_eq!(gitlab(text), text);
    }
}

#[test]
fn bare_urls_become_links_without_their_punctuation() {
    assert_eq!(
        gitlab("See https://a.io/b_c/-/issues/1#note_2, (http://a.io/x)."),
        "See <https://a.io/b_c/-/issues/1#note_2>, (<http://a.io/x>)."
    );
    assert_eq!(gitlab("https://a.io/x_(y)"), "<https://a.io/x_(y)>");
}

#[test]
fn a_link_keeps_its_references_as_text() {
    assert_eq!(gitlab("[fix #12](https://a.io)"), "[fix #12](https://a.io)");
    assert_eq!(gitlab("<https://a.io/#12>"), "<https://a.io/#12>");
}

#[test]
fn relative_links_resolve_on_the_project() {
    for (dest, url) in [
        ("/uploads/a/f.pdf", "https://git.io/org/app/uploads/a/f.pdf"),
        ("/org/app/-/issues/1", "https://git.io/org/app/-/issues/1"),
        (
            "./docs/a.md",
            "https://git.io/org/app/-/blob/main/docs/a.md",
        ),
        ("docs/a.md", "https://git.io/org/app/-/blob/main/docs/a.md"),
        ("#why", "https://git.io/org/app/-/merge_requests/7#why"),
        ("//b.io/c", "https://b.io/c"),
    ] {
        assert_eq!(gitlab(&format!("[x]({dest})")), format!("[x]({url})"));
    }
    assert_eq!(
        gitlab("[x][1]\n\n[1]: /uploads/a.pdf"),
        "[x][1]\n\n[1]: https://git.io/org/app/uploads/a.pdf"
    );
}

#[test]
fn links_that_must_not_open_lose_their_address() {
    for dest in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "data:text/html,x",
    ] {
        assert_eq!(gitlab(&format!("[click]({dest})")), "click");
    }
    assert_eq!(gitlab("[m](mailto:a@b.io)"), "[m](mailto:a@b.io)");
}

#[test]
fn images_become_links() {
    assert_eq!(
        gitlab("![shot](/uploads/a/s.png){width=100 height=50} after"),
        "[shot](<https://git.io/org/app/uploads/a/s.png>) after"
    );
    assert_eq!(
        gitlab("![](/uploads/a/s.png)"),
        "[s.png](<https://git.io/org/app/uploads/a/s.png>)"
    );
    // Not left as a link in a link.
    assert_eq!(
        gitlab("[![badge](https://a.io/b.svg)](https://a.io)"),
        "[badge](https://a.io)"
    );
    assert_eq!(gitlab("![x](data:image/png;base64,AA)"), "x");
}

#[test]
fn html_comments_disappear() {
    assert_eq!(gitlab("<!-- a\n\nb -->\nText"), "\nText");
    assert_eq!(gitlab("a <!-- b --> c"), "a  c");
}

#[test]
fn details_keep_their_content() {
    assert_eq!(
        gitlab("<details>\n  <summary> <b>Logs</b> #1 </summary>\n\n```\n<b>\n```\n\n</details>"),
        "\n**Logs [#1](<https://git.io/org/app/-/issues/1>)**\n\n\n\n```\n<b>\n```\n\n\n"
    );
}

#[test]
fn inline_tags_translate() {
    assert_eq!(gitlab("a<br>b"), "a  \nb");
    assert_eq!(gitlab("<b>a</b> <kbd>#1</kbd>"), "**a** `#1`");
    assert_eq!(
        gitlab("x <a href=\"/uploads/f.pdf\">f #1</a>"),
        "x [f #1](<https://git.io/org/app/uploads/f.pdf>)"
    );
    assert_eq!(
        gitlab("x <img src=\"/uploads/f.png\" width=\"10\" alt='f'> y"),
        "x [f](<https://git.io/org/app/uploads/f.png>) y"
    );
    assert_eq!(gitlab("a < b <c"), "a < b <c");
}

#[test]
fn a_table_stays_on_its_lines() {
    assert_eq!(gitlab("| a |\n|---|\n| b<br>c |"), "| a |\n|---|\n| b c |");
}

#[test]
fn indented_html_does_not_turn_into_code() {
    assert_eq!(gitlab("<div>\n      text\n</div>"), "\ntext\n\n\n");
}

fn text(text: &str) -> Block {
    Block::Text(format!("{text}\n\n"))
}

fn task(checked: bool, body: Vec<Block>) -> Item {
    Item {
        marker: Marker::Task { checked },
        body,
    }
}

#[test]
fn a_list_of_tasks_is_cut_out_of_the_text() {
    assert_eq!(
        blocks("Before\n\n- [x] a\n- [ ] b\n\nAfter", ""),
        vec![
            text("Before"),
            Block::List(vec![
                task(true, vec![text("a")]),
                task(false, vec![text("b")]),
            ]),
            text("After"),
        ]
    );
}

#[test]
fn plain_lists_and_quotes_stay_for_the_viewer() {
    let source = "- a\n- b\n\n> - [ ] c";
    assert_eq!(blocks(source, ""), vec![text(source)]);
}

#[test]
fn an_item_keeps_what_is_nested_in_it() {
    assert_eq!(
        blocks("1. first\n   - [x] sub\n\n     more\n2. [ ] second", ""),
        vec![Block::List(vec![
            Item {
                marker: Marker::Number(1),
                body: vec![
                    text("first"),
                    Block::List(vec![task(true, vec![text("sub\n\nmore")])]),
                ],
            },
            task(false, vec![text("second")]),
        ])]
    );
}

#[test]
fn a_reference_link_finds_its_definition_in_every_block() {
    let source = "- [ ] [x][1]\n\n[y][1]\n\n[1]: https://a.io";
    let definitions = definitions(source);
    assert_eq!(definitions, "[1]: https://a.io");
    assert_eq!(
        blocks(source, &definitions),
        vec![
            Block::List(vec![task(
                false,
                vec![Block::Text("[x][1]\n\n[1]: https://a.io".into())],
            )]),
            Block::Text("[y][1]\n\n[1]: https://a.io\n\n[1]: https://a.io".into()),
        ]
    );
}

#[test]
fn scripts_are_dropped() {
    assert_eq!(gitlab("<script>\nalert(1)\n</script>\n\nb"), "\n\nb");
}

#[test]
fn an_unusable_url_keeps_only_absolute_links() {
    let links = Links::new("", "main");
    assert_eq!(
        prepare("#1 [a](/b) [c](https://d.io)", &links),
        "#1 a [c](https://d.io)"
    );
}
