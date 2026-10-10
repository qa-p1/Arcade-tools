//! Release bodies and docs share one HTML5 sanitizer. Rendering itself performs
//! no disk/network I/O; run large documents on a worker thread.
use ammonia::{Builder, UrlRelative};
use pulldown_cmark::{html, Options, Parser};
use reqwest::Url;
use std::collections::{HashMap, HashSet};

/// Render CommonMark plus tables/strikethrough and sanitize the result.
/// CPU-only; use a worker for large input. Images from remote hosts are denied
/// unless the caller explicitly allows the exact host; an empty list is offline.
/// The caller controls navigation and serves relative images from its docs root.
pub fn render(markdown: &str, image_hosts: &[&str]) -> String {
    let parser = Parser::new_ext(
        markdown,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
    );
    let mut output = String::new();
    html::push_html(&mut output, parser);
    let hosts = image_hosts
        .iter()
        .map(|h| h.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let mut builder = Builder::new();
    builder
        .tags(
            [
                "h1",
                "h2",
                "h3",
                "h4",
                "h5",
                "h6",
                "p",
                "ul",
                "ol",
                "li",
                "code",
                "pre",
                "blockquote",
                "table",
                "thead",
                "tbody",
                "tr",
                "th",
                "td",
                "a",
                "img",
                "em",
                "strong",
                "del",
                "br",
                "hr",
            ]
            .into_iter()
            .collect(),
        )
        .generic_attributes(HashSet::new())
        .tag_attributes(HashMap::from([
            ("a", HashSet::from(["href", "title"])),
            ("img", HashSet::from(["src", "alt", "title"])),
            ("ol", HashSet::from(["start"])),
        ]))
        .clean_content_tags(HashSet::from([
            "script", "style", "svg", "math", "iframe", "object", "embed", "template",
        ]))
        .url_schemes(HashSet::from(["http", "https"]))
        .url_relative(UrlRelative::PassThrough)
        .link_rel(Some("noopener noreferrer"))
        .attribute_filter(move |element, attribute, value| {
            if matches!((element, attribute), ("a", "href") | ("img", "src"))
                && !safe_url(value, (element == "img").then_some(&hosts))
            {
                return None;
            }
            Some(value.into())
        });
    builder.clean(&output).to_string()
}
fn safe_url(value: &str, image_hosts: Option<&HashSet<String>>) -> bool {
    if value.is_empty()
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || value.trim() != value
        || value.starts_with("//")
    {
        return false;
    }
    match Url::parse(value) {
        Ok(url) => {
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && image_hosts
                    .is_none_or(|hosts| url.host_str().is_some_and(|host| hosts.contains(host)))
        }
        Err(_) => Url::parse("https://arcade-docs.invalid/")
            .unwrap()
            .join(value)
            .is_ok_and(|url| {
                url.scheme() == "https" && url.host_str() == Some("arcade-docs.invalid")
            }),
    }
}
