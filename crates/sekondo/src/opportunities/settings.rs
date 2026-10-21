//! The filters of one account: what they default to, how a submitted form is validated, and how
//! they are stored.

use uuid::Uuid;

use crate::store::{OpportunityStore, StoreError};

/// The longest item name search that is accepted.
pub const SEARCH_MAX: usize = 100;
/// The largest ISK figure that is accepted: far beyond any real market.
const ISK_MAX: f64 = 1e15;

/// The filters of one account, with the defaults of an account that never saved any.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Fraction: 0.05 is 5%.
    pub min_margin: f64,
    /// Fraction: 0.80 is 80%.
    pub max_margin: f64,
    pub min_isk_per_day: f64,
    pub search: String,
    pub capital: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            min_margin: 0.05,
            max_margin: 0.80,
            min_isk_per_day: 5_000_000.0,
            search: String::new(),
            capital: 100_000_000.0,
        }
    }
}

/// What a submitted filter form said, as text: kept as typed so a rejected form shows it again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Form {
    /// Percent: `5` is 5%.
    pub min_margin: String,
    pub max_margin: String,
    pub min_isk_per_day: String,
    pub search: String,
    pub capital: String,
}

impl Form {
    /// The form of stored settings, in the notation a person types.
    pub fn of(settings: &Settings) -> Self {
        Self {
            min_margin: plain(settings.min_margin * 100.0),
            max_margin: plain(settings.max_margin * 100.0),
            min_isk_per_day: plain(settings.min_isk_per_day),
            search: settings.search.clone(),
            capital: plain(settings.capital),
        }
    }

    pub fn from_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut form = Self::default();
        for (key, value) in pairs {
            match key.as_str() {
                "min_margin" => form.min_margin = value,
                "max_margin" => form.max_margin = value,
                "min_isk_per_day" => form.min_isk_per_day = value,
                "search" => form.search = value,
                "capital" => form.capital = value,
                _ => {}
            }
        }
        form
    }

    /// The settings the form asks for, or the first rejected field with the message that says what
    /// is wrong with it.
    pub fn validate(&self) -> Result<Settings, FieldError> {
        let margin = number(&self.min_margin)
            .filter(|m| *m >= 0.0)
            .ok_or_else(|| {
                FieldError::new(
                    Field::MinMargin,
                    "The minimum margin must be a number of at least 0 percent",
                )
            })?;
        let max_margin = number(&self.max_margin)
            .filter(|m| *m >= 0.0)
            .ok_or_else(|| {
                FieldError::new(
                    Field::MaxMargin,
                    "The maximum margin must be a number of at least 0 percent",
                )
            })?;
        if max_margin < margin {
            return Err(FieldError::new(
                Field::MaxMargin,
                "The maximum margin must be at least the minimum margin",
            ));
        }
        let min_isk = number(&self.min_isk_per_day)
            .filter(|v| *v >= 0.0)
            .ok_or_else(|| {
                FieldError::new(
                    Field::MinIskPerDay,
                    "The minimum ISK/day must be a number of at least 0",
                )
            })?;
        let capital = number(&self.capital).filter(|v| *v > 0.0).ok_or_else(|| {
            FieldError::new(
                Field::Capital,
                "The capital per item must be a positive number of ISK",
            )
        })?;
        let search = self.search.trim();
        if search.chars().count() > SEARCH_MAX {
            return Err(FieldError::new(
                Field::Search,
                format!("The item name search can have at most {SEARCH_MAX} characters"),
            ));
        }
        Ok(Settings {
            min_margin: margin / 100.0,
            max_margin: max_margin / 100.0,
            min_isk_per_day: min_isk,
            search: search.to_owned(),
            capital,
        })
    }
}

/// One of the five filter inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    MinMargin,
    MaxMargin,
    MinIskPerDay,
    Search,
    Capital,
}

impl Field {
    /// The name of the field's input in the form.
    pub fn name(self) -> &'static str {
        match self {
            Self::MinMargin => "min_margin",
            Self::MaxMargin => "max_margin",
            Self::MinIskPerDay => "min_isk_per_day",
            Self::Search => "search",
            Self::Capital => "capital",
        }
    }
}

/// A value the form is rejected for: the field that holds it and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: Field,
    pub message: String,
}

impl FieldError {
    fn new(field: Field, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

/// A plain decimal number as a person types it: digits with an optional decimal point and
/// optional thousands separators (commas, spaces, underscores). Anything else, and anything
/// beyond [`ISK_MAX`], is not a number here.
fn number(text: &str) -> Option<f64> {
    let cleaned: String = text
        .trim()
        .chars()
        .filter(|c| !matches!(c, ',' | ' ' | '_'))
        .collect();
    let unsigned = cleaned.strip_prefix('-').unwrap_or(&cleaned);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let digits = |part: &str| part.chars().all(|c| c.is_ascii_digit());
    if whole.is_empty() && fraction.is_empty() || !digits(whole) || !digits(fraction) {
        return None;
    }
    let value: f64 = cleaned.parse().ok()?;
    (value.is_finite() && value.abs() <= ISK_MAX).then_some(value)
}

/// A float without a trailing `.0`: `5`, `7.5`, `100000000`.
fn plain(value: f64) -> String {
    let text = format!("{value:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl Settings {
    /// The account's saved filters, or the defaults when it never saved any.
    pub async fn load(store: &dyn OpportunityStore, account_id: Uuid) -> Result<Self, StoreError> {
        Ok(store.load_settings(account_id).await?.unwrap_or_default())
    }

    pub async fn save(
        &self,
        store: &dyn OpportunityStore,
        account_id: Uuid,
    ) -> Result<(), StoreError> {
        store.save_settings(account_id, self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(margin: &str, isk: &str, search: &str, capital: &str) -> Form {
        Form {
            min_margin: margin.to_owned(),
            max_margin: "80".to_owned(),
            min_isk_per_day: isk.to_owned(),
            search: search.to_owned(),
            capital: capital.to_owned(),
        }
    }

    fn form_with_max(margin: &str, max: &str) -> Form {
        Form {
            max_margin: max.to_owned(),
            ..form(margin, "1", "", "1")
        }
    }

    #[test]
    fn the_default_maximum_margin_is_eighty_percent() {
        assert_eq!(Settings::default().max_margin, 0.80);
        assert_eq!(Form::of(&Settings::default()).max_margin, "80");
    }

    #[test]
    fn a_valid_maximum_margin_is_a_fraction_and_may_equal_the_minimum() {
        let settings = form_with_max("10", "150").validate().unwrap();
        assert_eq!(settings.max_margin, 1.5);
        assert!(form_with_max("10", "10").validate().is_ok());
    }

    #[test]
    fn a_rejected_maximum_margin_names_its_field_and_says_what_is_wrong() {
        for max in ["", "-1", "abc"] {
            let err = form_with_max("10", max).validate().unwrap_err();
            assert_eq!(err.field, Field::MaxMargin, "{max:?}");
            assert_eq!(
                err.message, "The maximum margin must be a number of at least 0 percent",
                "{max:?}"
            );
        }
        let err = form_with_max("10", "5").validate().unwrap_err();
        assert_eq!(err.field, Field::MaxMargin);
        assert_eq!(
            err.message,
            "The maximum margin must be at least the minimum margin"
        );
    }

    #[test]
    fn the_maximum_margin_is_checked_after_the_minimum_and_before_the_rest() {
        let err = form("-1", "-1", "", "-1").validate().unwrap_err();
        assert_eq!(err.field, Field::MinMargin);
        let mut both = form("5", "-1", "", "-1");
        both.max_margin = "-1".to_owned();
        assert_eq!(both.validate().unwrap_err().field, Field::MaxMargin);
        assert_eq!(Field::MaxMargin.name(), "max_margin");
    }

    #[test]
    fn the_defaults_are_five_percent_five_million_nothing_and_a_hundred_million() {
        let defaults = Settings::default();
        assert_eq!(defaults.min_margin, 0.05);
        assert_eq!(defaults.min_isk_per_day, 5_000_000.0);
        assert_eq!(defaults.search, "");
        assert_eq!(defaults.capital, 100_000_000.0);
    }

    #[test]
    fn the_default_form_shows_the_defaults_as_a_person_types_them() {
        assert_eq!(
            Form::of(&Settings::default()),
            form("5", "5000000", "", "100000000")
        );
        assert_eq!(Form::of(&Settings::default()).max_margin, "80");
        let odd = Settings {
            min_margin: 0.075,
            ..Settings::default()
        };
        assert_eq!(Form::of(&odd).min_margin, "7.5");
    }

    #[test]
    fn a_valid_form_gives_settings_with_the_margin_as_a_fraction() {
        let settings = form("7.5", "10,000,000", "  pyer ", "2 000 000 000")
            .validate()
            .unwrap();
        assert_eq!(settings.min_margin, 0.075);
        assert_eq!(settings.min_isk_per_day, 10_000_000.0);
        assert_eq!(settings.search, "pyer");
        assert_eq!(settings.capital, 2_000_000_000.0);
    }

    #[test]
    fn zero_is_fine_for_the_margin_and_the_isk_per_day_but_not_for_the_capital() {
        assert!(form("0", "0", "", "1").validate().is_ok());
        let err = form("5", "5000000", "", "0").validate().unwrap_err();
        assert_eq!(err.field, Field::Capital);
        assert!(
            err.message.contains("capital") && err.message.contains("positive"),
            "{err:?}"
        );
    }

    #[test]
    fn each_rejected_value_names_its_own_field_and_says_what_is_wrong() {
        let cases = [
            (form("-1", "1", "", "1"), Field::MinMargin, "margin"),
            (form("abc", "1", "", "1"), Field::MinMargin, "margin"),
            (form("", "1", "", "1"), Field::MinMargin, "margin"),
            (form("5", "-1", "", "1"), Field::MinIskPerDay, "ISK/day"),
            (form("5", "1e6", "", "1"), Field::MinIskPerDay, "ISK/day"),
            (form("5", "1", "", "-5"), Field::Capital, "capital"),
            (form("5", "1", "", "NaN"), Field::Capital, "capital"),
            (form("5", "1", "", "inf"), Field::Capital, "capital"),
            (form("5", "1", "", "1.2.3"), Field::Capital, "capital"),
            (
                form("5", "1", "", "99999999999999999999"),
                Field::Capital,
                "capital",
            ),
            (form("5", "1", "", ""), Field::Capital, "capital"),
        ];
        for (input, field, word) in cases {
            let err = input.validate().unwrap_err();
            assert_eq!(err.field, field, "{input:?}: {err:?}");
            assert!(err.message.contains(word), "{input:?}: {err:?}");
        }
    }

    #[test]
    fn the_fields_are_named_as_their_inputs() {
        assert_eq!(Field::MinMargin.name(), "min_margin");
        assert_eq!(Field::MinIskPerDay.name(), "min_isk_per_day");
        assert_eq!(Field::Search.name(), "search");
        assert_eq!(Field::Capital.name(), "capital");
    }

    #[test]
    fn the_first_rejected_field_is_the_one_reported() {
        let err = form("-1", "-1", "", "-1").validate().unwrap_err();
        assert_eq!(err.field, Field::MinMargin);
    }

    #[test]
    fn an_overlong_search_is_rejected() {
        let long = "x".repeat(SEARCH_MAX + 1);
        let err = form("5", "1", &long, "1").validate().unwrap_err();
        assert_eq!(err.field, Field::Search);
        assert!(err.message.contains("search"), "{err:?}");
        let fits = "x".repeat(SEARCH_MAX);
        assert!(form("5", "1", &fits, "1").validate().is_ok());
    }

    #[test]
    fn a_form_is_read_from_pairs_and_ignores_the_rest() {
        let pairs = [
            ("min_margin", "6"),
            ("capital", "50"),
            ("chars", "1,2"),
            ("cs", "item"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()));
        let form = Form::from_pairs(pairs);
        assert_eq!(
            (form.min_margin.as_str(), form.capital.as_str()),
            ("6", "50")
        );
        assert_eq!(form.search, "");
    }
}
