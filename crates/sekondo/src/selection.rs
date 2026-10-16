//! Which of the account's characters a view shows, as carried by the `chars` query parameter.

use axum::{extract::FromRequestParts, http::request::Parts};

/// The query string every character-scoped view accepts. Sorting is parsed by the view that owns
/// the table (`orders`) and the range by the dashboard; this holds the raw values so one place carries them all.
#[derive(Debug, Default, Clone)]
pub struct ViewQuery {
    pub chars: Option<String>,
    pub sort: Option<String>,
    pub dir: Option<String>,
    pub bsort: Option<String>,
    pub bdir: Option<String>,
    pub range: Option<String>,
}

impl ViewQuery {
    /// Never fails: unknown keys are ignored, and the first of a repeated key wins.
    pub fn parse(query: Option<&str>) -> Self {
        let mut parsed = Self::default();
        for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
            let slot = match key.as_ref() {
                "chars" => &mut parsed.chars,
                "sort" => &mut parsed.sort,
                "dir" => &mut parsed.dir,
                "bsort" => &mut parsed.bsort,
                "bdir" => &mut parsed.bdir,
                "range" => &mut parsed.range,
                _ => continue,
            };
            slot.get_or_insert_with(|| value.into_owned());
        }
        parsed
    }

    pub fn selection(&self, account_characters: &[i64]) -> CharacterSelection {
        CharacterSelection::resolve(account_characters, self.chars.as_deref())
    }
}

impl<S: Send + Sync> FromRequestParts<S> for ViewQuery {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self::parse(parts.uri.query()))
    }
}

/// A non-empty subset of the account's characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterSelection {
    ids: Vec<i64>,
    /// Every character of the account is selected: the URL then carries no `chars`.
    all: bool,
}

impl CharacterSelection {
    /// The only place `chars` is parsed. Anything that is not a character of the account (foreign,
    /// malformed or duplicated) is dropped without a trace, and an empty result means "all".
    pub fn resolve(account_characters: &[i64], raw_chars: Option<&str>) -> Self {
        let requested: Vec<i64> = raw_chars
            .unwrap_or_default()
            .split(',')
            .filter_map(|part| part.trim().parse::<i64>().ok())
            .collect();
        // Keeps the account's order, which also removes duplicates.
        let ids: Vec<i64> = account_characters
            .iter()
            .copied()
            .filter(|id| requested.contains(id))
            .collect();
        if ids.is_empty() {
            return Self::all_of(account_characters);
        }
        let all = ids.len() == account_characters.len();
        Self { ids, all }
    }

    fn all_of(account_characters: &[i64]) -> Self {
        Self {
            ids: account_characters.to_vec(),
            all: true,
        }
    }

    pub fn ids(&self) -> &[i64] {
        &self.ids
    }

    pub fn contains(&self, id: i64) -> bool {
        self.ids.contains(&id)
    }

    /// The `chars` value for links, or `None` when everything is selected.
    pub fn param(&self) -> Option<String> {
        (!self.all).then(|| {
            self.ids
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        })
    }

    /// `""` or `?chars=<ids>`, for links to views that accept the filter.
    pub fn query(&self) -> String {
        self.param()
            .map_or_else(String::new, |chars| format!("?chars={chars}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: [i64; 3] = [1, 2, 3];

    fn resolve(raw: Option<&str>) -> CharacterSelection {
        CharacterSelection::resolve(&OWN, raw)
    }

    #[test]
    fn a_missing_or_empty_parameter_selects_everything() {
        for raw in [None, Some(""), Some(" "), Some(",")] {
            let s = resolve(raw);
            assert_eq!(s.ids(), &OWN, "{raw:?}");
            assert_eq!(s.param(), None);
            assert_eq!(s.query(), "");
        }
    }

    #[test]
    fn a_subset_is_kept_in_the_accounts_order() {
        let s = resolve(Some("3,1"));
        assert_eq!(s.ids(), &[1, 3]);
        assert_eq!(s.param().as_deref(), Some("1,3"));
        assert_eq!(s.query(), "?chars=1,3");
        assert!(s.contains(3) && !s.contains(2));
    }

    #[test]
    fn selecting_every_character_is_the_same_as_no_filter() {
        let s = resolve(Some("1,2,3"));
        assert_eq!(s.ids(), &OWN);
        assert_eq!(s.param(), None);
    }

    #[test]
    fn a_foreign_id_alone_falls_back_to_everything() {
        let s = resolve(Some("999"));
        assert_eq!(s.ids(), &OWN);
        assert_eq!(s.param(), None);
    }

    #[test]
    fn malformed_values_are_ignored() {
        assert_eq!(resolve(Some("abc")).ids(), &OWN);
        assert_eq!(resolve(Some("1.5,;2")).ids(), &OWN);
        assert_eq!(resolve(Some("99999999999999999999999")).ids(), &OWN);
        assert_eq!(resolve(Some("1,abc")).ids(), &[1]);
    }

    #[test]
    fn mixed_valid_foreign_and_malformed_ids_keep_only_the_own_ones() {
        let s = resolve(Some("2,999,abc"));
        assert_eq!(s.ids(), &[2]);
        assert_eq!(s.query(), "?chars=2");
    }

    #[test]
    fn duplicates_and_spaces_are_harmless() {
        assert_eq!(resolve(Some("2, 2 ,2")).ids(), &[2]);
    }

    #[test]
    fn the_query_string_is_parsed_leniently() {
        let q = ViewQuery::parse(Some(
            "chars=1%2C2&sort=price&dir=desc&bsort=item&bdir=asc&range=30d&x=1",
        ));
        assert_eq!(q.chars.as_deref(), Some("1,2"));
        assert_eq!(q.sort.as_deref(), Some("price"));
        assert_eq!(q.dir.as_deref(), Some("desc"));
        assert_eq!(q.bsort.as_deref(), Some("item"));
        assert_eq!(q.bdir.as_deref(), Some("asc"));
        assert_eq!(q.range.as_deref(), Some("30d"));
        // Repeated keys: the first wins. Garbage: ignored.
        let q = ViewQuery::parse(Some("chars=1&chars=2&%ff=%ff&&="));
        assert_eq!(q.chars.as_deref(), Some("1"));
        let q = ViewQuery::parse(None);
        assert!(q.chars.is_none() && q.sort.is_none());
    }

    #[test]
    fn the_view_query_resolves_its_chars() {
        let q = ViewQuery {
            chars: Some("1,2".to_owned()),
            ..ViewQuery::default()
        };
        assert_eq!(q.selection(&OWN).ids(), &[1, 2]);
        assert_eq!(ViewQuery::default().selection(&OWN).ids(), &OWN);
    }
}
