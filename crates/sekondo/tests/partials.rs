//! The partials every table page shares: KPI cards and the pagination bar.

use askama::Template;
use sekondo::{
    kpi::{Kpi, KpiLink},
    pagination::{Link, Paging, paginate},
};

#[derive(Template)]
#[template(source = r#"{% include "kpi_card.html" %}"#, ext = "html")]
struct Card {
    kpi: Kpi,
}

#[derive(Template)]
#[template(source = r#"{% include "pagination.html" %}"#, ext = "html")]
struct Bar {
    pager: sekondo::pagination::PageView,
}

fn card(kpi: Kpi) -> String {
    Card { kpi }.render().unwrap()
}

fn link(paging: Paging) -> Link {
    Link {
        href: format!("/orders/sell?{}", paging.query()),
        content: format!("/orders/sell/content?{}", paging.query()),
    }
}

fn bar(rows: usize, paging: Paging) -> String {
    let items = vec![0u8; rows];
    let (_, pager) = paginate(&items, paging, link);
    Bar { pager }.render().unwrap()
}

#[test]
fn a_card_shows_its_name_and_value_with_the_exact_value_on_hover() {
    let html = card(Kpi::new("Sell value", "1.5m", "1,500,000.00"));

    assert!(html.contains(r#"<article class="kpi">"#), "{html}");
    assert!(html.contains("Sell value"));
    assert!(html.contains(r#"title="1,500,000.00""#));
    assert!(html.contains(">1.5m<"));
    assert!(
        !html.contains("<small"),
        "no detail line without a detail: {html}"
    );
    assert!(!html.contains("<a "), "no link without a link: {html}");
}

#[test]
fn a_card_can_carry_a_detail_line_a_link_an_id_and_the_error_style() {
    let kpi = Kpi::new("Margin", "-3.0k", "-3,000.00")
        .with_id("kpi-margin")
        .with_detail("2 left out\nof the total")
        .with_link(KpiLink::new("/wealth", "Details"))
        .as_error();

    let html = card(kpi);

    assert!(html.contains(r#"class="kpi error""#), "{html}");
    assert!(html.contains(r#"id="kpi-margin""#));
    assert!(
        html.contains(r#"<small class="detail" title="2 left out"#),
        "{html}"
    );
    assert!(html.contains(r#"<a href="/wealth">Details</a>"#));
}

#[test]
fn a_log_in_again_link_carries_the_icon_and_its_text() {
    let html = card(Kpi::new("Sales tax", "3.38%", "").with_link(KpiLink::log_in_again()));

    assert!(html.contains(r#"<a href="/auth/add""#), "{html}");
    assert!(html.contains("<svg"), "{html}");
    assert!(html.contains("Log in again</a>"), "{html}");
}

#[test]
fn the_pagination_bar_links_previous_and_next_through_the_content_fragment() {
    let html = bar(
        120,
        Paging {
            page: 2,
            per_page: 50,
        },
    );

    assert!(html.contains("Page 2 of 3"), "{html}");
    for (target, page) in [("/orders/sell?", "prev"), ("/orders/sell?page=3", "next")] {
        let anchor = html
            .split("<a ")
            .find(|a| a.contains(&format!(r#"rel="{page}""#)))
            .unwrap_or_else(|| panic!("no {page} link: {html}"));
        assert!(anchor.contains(&format!(r#"href="{target}""#)), "{anchor}");
        assert!(
            anchor.contains(&format!(r#"hx-push-url="{target}""#)),
            "{anchor}"
        );
        assert!(anchor.contains(r##"hx-target="#content""##), "{anchor}");
        assert!(anchor.contains("show:#content:top"), "{anchor}");
    }
    assert!(
        html.contains(r#"hx-get="/orders/sell/content?page=3""#),
        "{html}"
    );
}

#[test]
fn the_first_page_cannot_go_back_and_the_last_cannot_go_on() {
    let first = bar(
        120,
        Paging {
            page: 1,
            per_page: 50,
        },
    );
    let last = bar(
        120,
        Paging {
            page: 3,
            per_page: 50,
        },
    );

    assert!(!first.contains(r#"rel="prev""#));
    assert!(first.contains(r#"rel="next""#));
    assert!(last.contains(r#"rel="prev""#));
    assert!(!last.contains(r#"rel="next""#));
}

#[test]
fn the_page_size_select_offers_the_four_sizes_and_marks_the_current_one() {
    let html = bar(
        300,
        Paging {
            page: 1,
            per_page: 100,
        },
    );

    assert!(html.contains("<select"), "{html}");
    for size in ["25", "50", "100", "200"] {
        assert!(
            html.contains(&format!(r#"<option value="{size}""#)),
            "{size}: {html}"
        );
    }
    assert!(html.contains(r#"<option value="100" data-href="/orders/sell?per_page=100" data-content="/orders/sell/content?per_page=100" selected>"#), "{html}");
}

#[test]
fn no_rows_means_no_pagination_bar() {
    assert_eq!(bar(0, Paging::default()).trim(), "");
}
