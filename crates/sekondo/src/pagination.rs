//! Paging of the lists the table pages show: parsed from `page` and `per_page`, applied after the
//! list is filtered and sorted, and described for the controls under the table.

/// The page sizes the page-size control offers.
pub const PER_PAGE_SIZES: [u32; 5] = [10, 25, 50, 100, 200];
pub const DEFAULT_PER_PAGE: u32 = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paging {
    pub page: u32,
    pub per_page: u32,
}

impl Default for Paging {
    fn default() -> Self {
        Self {
            page: 1,
            per_page: DEFAULT_PER_PAGE,
        }
    }
}

impl Paging {
    /// Never fails: a missing, malformed or non-positive `page` is 1, and a `per_page` that is not
    /// one of the offered sizes is the default.
    pub fn parse(page: Option<&str>, per_page: Option<&str>) -> Self {
        Self::parse_or(page, per_page, DEFAULT_PER_PAGE)
    }

    /// Like [`Paging::parse`], but a missing `per_page` is `fallback`, the size a page keeps for
    /// its user. A `per_page` that is present and not one of the offered sizes is still the default.
    pub fn parse_or(page: Option<&str>, per_page: Option<&str>, fallback: u32) -> Self {
        let page = page
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|p| *p >= 1)
            .unwrap_or(1);
        let per_page = match per_page {
            None => fallback,
            Some(v) => v
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|n| PER_PAGE_SIZES.contains(n))
                .unwrap_or(DEFAULT_PER_PAGE),
        };
        Self { page, per_page }
    }

    /// `page=…&per_page=…` with the defaults left out; `""` when both are default.
    pub fn query(self) -> String {
        self.query_or(DEFAULT_PER_PAGE)
    }

    /// Like [`Paging::query`], but a `per_page` equal to `fallback` is left out: the page falls
    /// back to it when the URL says nothing.
    pub fn query_or(self, fallback: u32) -> String {
        let mut pairs = Vec::new();
        if self.page != 1 {
            pairs.push(format!("page={}", self.page));
        }
        if self.per_page != fallback {
            pairs.push(format!("per_page={}", self.per_page));
        }
        pairs.join("&")
    }

    /// The same page size, from the first page: for links that change what the list holds.
    pub fn first(self) -> Self {
        Self { page: 1, ..self }
    }
}

/// Where a control leads: the page's URL (for the address bar and for plain links) and the URL
/// of the content fragment that htmx fetches for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub href: String,
    pub content: String,
}

/// One entry of the page-size control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeOption {
    pub value: u32,
    pub selected: bool,
    pub link: Link,
}

/// What the controls under a table need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageView {
    /// The paging actually used, after clamping.
    pub paging: Paging,
    pub total: usize,
    /// "Page x of y".
    pub label: String,
    pub prev: Option<Link>,
    pub next: Option<Link>,
    pub sizes: Vec<SizeOption>,
    /// Where the page-size control posts the chosen size to keep it, for pages that store it.
    pub save: Option<String>,
}

/// Cuts the page out of an already filtered and sorted list. `href` builds the URL of the view
/// for a paging; the controls link to it.
pub fn paginate<T>(items: &[T], paging: Paging, href: impl Fn(Paging) -> Link) -> (&[T], PageView) {
    let per_page = paging.per_page as usize;
    let last = items.len().div_ceil(per_page).max(1);
    let page = (paging.page as usize).clamp(1, last);
    let paging = Paging {
        page: page as u32,
        per_page: paging.per_page,
    };
    let start = (page - 1) * per_page;
    let slice = &items[start.min(items.len())..(start + per_page).min(items.len())];
    let at = |page: usize| {
        href(Paging {
            page: page as u32,
            ..paging
        })
    };
    let view = PageView {
        paging,
        total: items.len(),
        label: format!("Page {page} of {last}"),
        prev: (page > 1).then(|| at(page - 1)),
        next: (page < last).then(|| at(page + 1)),
        sizes: PER_PAGE_SIZES
            .iter()
            .map(|&value| SizeOption {
                value,
                selected: value == paging.per_page,
                link: href(Paging {
                    page: 1,
                    per_page: value,
                }),
            })
            .collect(),
        save: None,
    };
    (slice, view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(page: &str, per_page: &str) -> Paging {
        Paging::parse(Some(page), Some(per_page))
    }

    fn href(paging: Paging) -> Link {
        Link {
            href: format!("/list?{}", paging.query()),
            content: format!("/list/content?{}", paging.query()),
        }
    }

    #[test]
    fn missing_values_give_page_one_of_twenty_five() {
        assert_eq!(
            Paging::parse(None, None),
            Paging {
                page: 1,
                per_page: 25
            }
        );
    }

    #[test]
    fn the_sizes_are_ten_twenty_five_fifty_a_hundred_and_two_hundred() {
        assert_eq!(PER_PAGE_SIZES, [10, 25, 50, 100, 200]);
    }

    #[test]
    fn a_missing_per_page_is_the_fallback_and_an_unknown_one_is_the_default() {
        assert_eq!(Paging::parse_or(None, None, 100).per_page, 100);
        assert_eq!(Paging::parse_or(None, Some("50"), 100).per_page, 50);
        assert_eq!(Paging::parse_or(None, Some("30"), 100).per_page, 25);
        assert_eq!(Paging::parse_or(None, Some(""), 100).per_page, 25);
        assert_eq!(Paging::parse_or(Some("2"), None, 10).page, 2);
    }

    #[test]
    fn the_query_with_a_fallback_leaves_out_that_size_only() {
        let paging = |per_page| Paging { page: 1, per_page };
        assert_eq!(paging(100).query_or(100), "");
        assert_eq!(paging(25).query_or(100), "per_page=25");
        assert_eq!(paging(100).query(), "per_page=100");
        assert_eq!(
            Paging {
                page: 2,
                per_page: 100
            }
            .query_or(100),
            "page=2"
        );
    }

    #[test]
    fn valid_values_are_taken() {
        assert_eq!(
            parse("3", "100"),
            Paging {
                page: 3,
                per_page: 100
            }
        );
        for size in PER_PAGE_SIZES {
            assert_eq!(parse("1", &size.to_string()).per_page, size);
        }
    }

    #[test]
    fn malformed_or_non_positive_values_fall_back_to_the_defaults() {
        for page in ["abc", "0", "-4", "", "2.5", "99999999999999"] {
            assert_eq!(parse(page, "50").page, 1, "page {page:?}");
        }
        for size in ["abc", "0", "-25", "30", "1000", ""] {
            assert_eq!(parse("1", size).per_page, 25, "per_page {size:?}");
        }
    }

    #[test]
    fn the_query_omits_the_defaults() {
        assert_eq!(Paging::default().query(), "");
        assert_eq!(
            Paging {
                page: 2,
                per_page: 25
            }
            .query(),
            "page=2"
        );
        assert_eq!(
            Paging {
                page: 1,
                per_page: 100
            }
            .query(),
            "per_page=100"
        );
        assert_eq!(
            Paging {
                page: 3,
                per_page: 50
            }
            .query(),
            "page=3&per_page=50"
        );
    }

    #[test]
    fn a_page_past_the_end_is_clamped_to_the_last_page() {
        let items: Vec<u32> = (0..120).collect();

        let (slice, view) = paginate(&items, parse("9", "50"), href);

        assert_eq!(slice, &items[100..120]);
        assert_eq!(view.paging.page, 3);
        assert_eq!(view.label, "Page 3 of 3");
    }

    #[test]
    fn a_page_in_the_middle_links_both_ways_and_slices_exactly() {
        let items: Vec<u32> = (0..60).collect();

        let (slice, view) = paginate(&items, parse("2", "25"), href);

        assert_eq!(slice, &items[25..50]);
        assert_eq!(view.label, "Page 2 of 3");
        assert_eq!(view.total, 60);
        assert_eq!(view.prev.as_ref().map(|l| l.href.as_str()), Some("/list?"));
        assert_eq!(
            view.prev.as_ref().map(|l| l.content.as_str()),
            Some("/list/content?")
        );
        assert_eq!(
            view.next.as_ref().map(|l| l.href.as_str()),
            Some("/list?page=3")
        );
        assert_eq!(
            view.next.as_ref().map(|l| l.content.as_str()),
            Some("/list/content?page=3")
        );
    }

    #[test]
    fn the_first_page_has_no_previous_link_and_the_last_has_no_next_link() {
        let items: Vec<u32> = (0..120).collect();

        let (_, first) = paginate(&items, parse("1", "50"), href);
        let (_, last) = paginate(&items, parse("3", "50"), href);

        assert_eq!(first.prev, None);
        assert!(first.next.is_some());
        assert!(last.prev.is_some());
        assert_eq!(last.next, None);
    }

    #[test]
    fn an_empty_list_is_page_one_of_one() {
        let items: Vec<u32> = Vec::new();

        let (slice, view) = paginate(&items, parse("4", "50"), href);

        assert!(slice.is_empty());
        assert_eq!(view.label, "Page 1 of 1");
        assert_eq!((view.prev, view.next), (None, None));
    }

    #[test]
    fn changing_the_page_size_goes_back_to_page_one_and_marks_the_current_size() {
        let items: Vec<u32> = (0..300).collect();

        let (_, view) = paginate(&items, parse("3", "100"), href);

        let sizes: Vec<(u32, bool, &str)> = view
            .sizes
            .iter()
            .map(|s| (s.value, s.selected, s.link.href.as_str()))
            .collect();
        assert_eq!(
            sizes,
            [
                (10, false, "/list?per_page=10"),
                (25, false, "/list?"),
                (50, false, "/list?per_page=50"),
                (100, true, "/list?per_page=100"),
                (200, false, "/list?per_page=200"),
            ]
        );
    }
}
