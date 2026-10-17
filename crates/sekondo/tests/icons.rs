use askama::Template;

#[derive(Template)]
#[template(
    source = r#"{% include "icons/users.html" %}|{% include "icons/log-out.html" %}|{% include "icons/user-plus.html" %}|{% include "icons/unlink.html" %}|{% include "icons/log-in.html" %}|{% include "icons/panel-right-open.html" %}|{% include "icons/panel-right-close.html" %}"#,
    ext = "html"
)]
struct AllIcons;

#[test]
fn every_icon_is_an_inline_decorative_svg_in_the_text_colour() {
    let html = AllIcons.render().unwrap();
    let icons: Vec<&str> = html.split('|').collect();
    assert_eq!(icons.len(), 7);
    for icon in icons {
        let icon = icon.trim();
        assert!(
            icon.starts_with("<svg") && icon.ends_with("</svg>"),
            "{icon}"
        );
        assert!(icon.contains(r#"stroke="currentColor""#), "{icon}");
        assert!(
            icon.contains(r#"width="14""#) && icon.contains(r#"height="14""#),
            "{icon}"
        );
        assert!(icon.contains(r#"aria-hidden="true""#), "{icon}");
        assert!(
            !icon.contains("style="),
            "inline styles are blocked by the CSP: {icon}"
        );
    }
}
