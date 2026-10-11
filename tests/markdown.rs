use arcade_tools_core::markdown::render;

#[test]
fn keeps_markdown_tables_code_links_and_permitted_images() {
    let html = render("# Notes\n\n- One\n- Two\n\n> Quote\n\n```rust\nlet x = 1;\n```\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n[web](https://github.com/qa-p1) [next](getting-started.md)\n\n![local](images/test.png) ![remote](https://raw.githubusercontent.com/qa-p1/test/image.png)", &["raw.githubusercontent.com"]);
    for tag in [
        "<h1>",
        "<ul>",
        "<blockquote>",
        "<pre><code>",
        "<table>",
        "<thead>",
        "<tbody>",
    ] {
        assert!(html.contains(tag), "{html}");
    }
    assert!(html.contains("rel=\"noopener noreferrer\""));
    assert!(html.contains("src=\"images/test.png\""));
    assert!(html.contains("src=\"https://raw.githubusercontent.com/qa-p1/test/image.png\""));
}
#[test]
fn removes_xss_scripts_events_schemes_svg_css_and_frames() {
    for attack in [
        "<script>alert(1)</script>",
        "[bad](javascript:alert%281%29)",
        "<a href='JaVaScRiPt:alert(1)'>bad</a>",
        "<a href='&#106;avascript:alert(1)'>bad</a>",
        "<a href='java&#10;script:alert(1)'>bad</a>",
        "<img src=x onerror='alert(1)' onclick='alert(1)' style='background:url(javascript:alert(1))'>",
        "<img src='data:image/svg+xml;base64,PHN2Zz4='>",
        "![bad](data:image/png;base64,aaaa)",
        "<svg onload='alert(1)'><script>alert(1)</script><a xlink:href='javascript:alert(1)'>bad</a></svg>",
        "<iframe srcdoc='<script>alert(1)</script>'></iframe>",
        "<style>body{display:none}</style>",
        "<math><mtext><img src=x onerror='alert(1)'></mtext></math>",
    ] {
        let html = render(attack, &[]);
        for forbidden in ["<script", "javascript:", "onerror=", "onclick=", "onload=", "style=", "data:", "<svg", "<iframe", "<math", "<style"] {
            assert!(!html.to_lowercase().contains(forbidden), "attack: {attack}; output: {html}");
        }
    }
}
#[test]
fn image_hosts_are_exact_and_scheme_relative_and_unapproved_urls_are_removed() {
    let html = render("<img src='https://allowed.example/image.png'><img src='https://allowed.example.evil/image.png'><img src='//allowed.example/image.png'><img src='https://evil.example/image.png'><img src='file:///home/user/private.png'><img src='https://user@allowed.example/image.png'><a href='mailto:evil@example.com'>mail</a><a href='//evil.example/path'>relative host</a>", &["allowed.example"]);
    assert_eq!(html.matches("src=").count(), 1);
    assert!(html.contains("src=\"https://allowed.example/image.png\""));
    assert!(!html.contains("href="));
    assert!(!render("![remote](https://allowed.example/image.png)", &[]).contains("src="));
}
