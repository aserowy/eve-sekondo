//! What the market charges a character at each station or structure where it has orders: the
//! sales tax, the broker fee and the minimum margin of a station trade. One SQL statement works
//! them out, and both the orders page and the `fees-updated` change detection read it, so a
//! signal goes out exactly when what is shown changes (design §5).
//!
//! The values are `numeric` in the database and stay decimal text here: the crate has no decimal
//! type, and `f64` arithmetic would not be exact.

use std::collections::HashMap;

use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    esi::{EsiClient, EsiError},
    opportunities::{STATION_ID, STATION_NAME},
    store::{FeeStore, StoreError, Stores},
};

/// ESI's skill IDs of the two skills that matter.
pub const ACCOUNTING: i64 = 16622;
pub const BROKER_RELATIONS: i64 = 3446;

/// The fee figures of one character at one location, as decimal text in percent.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct FeeRow {
    pub character_id: i64,
    pub location_id: i64,
    pub is_structure: bool,
    pub accounting: Option<i32>,
    pub broker_relations: Option<i32>,
    pub faction_standing: Option<String>,
    pub corp_standing: Option<String>,
    pub sales_tax: Option<String>,
    pub broker_fee: Option<String>,
    pub min_margin: Option<String>,
}

/// The fees of a character at its fee location, and whether the missing location scope is why
/// that is Jita 4-4 (the page then offers to log in again).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterFee {
    pub row: FeeRow,
    pub needs_location_scope: bool,
}

/// What the pages show of the fees of these characters: the rates at the locations of their orders
/// and of their sales, and at their fee location. Two snapshots differ exactly when a signal is
/// due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeSnapshot {
    pub orders: Vec<FeeRow>,
    pub fee_location: Vec<CharacterFee>,
    pub sales: Vec<(i64, i64, Option<String>, Option<String>)>,
}

impl FeeSnapshot {
    /// The part of one character.
    pub fn of_character(&self, character_id: i64) -> Self {
        Self {
            orders: self
                .orders
                .iter()
                .filter(|r| r.character_id == character_id)
                .cloned()
                .collect(),
            fee_location: self
                .fee_location
                .iter()
                .filter(|r| r.row.character_id == character_id)
                .cloned()
                .collect(),
            sales: self
                .sales
                .iter()
                .filter(|r| r.0 == character_id)
                .cloned()
                .collect(),
        }
    }
}

/// Makes sure the owner of an NPC station is known: asks ESI once when it is not, and stores it
/// for every account. Answers whether the owner was learned just now.
pub async fn ensure_station_owner(
    store: &dyn FeeStore,
    esi: &EsiClient,
    station_id: i64,
    now: OffsetDateTime,
) -> Result<bool, EsiError> {
    let database = |e: StoreError| EsiError::Temporary(format!("database error: {e}"));
    if store
        .station_owner_known(station_id)
        .await
        .map_err(database)?
    {
        return Ok(false);
    }
    let station = esi.station(station_id).await?;
    store
        .insert_station_owner(station_id, station.owner, now)
        .await
        .map_err(database)?;
    Ok(true)
}

// ---- what the orders page shows ----

/// What the table shows for an unknown figure.
const UNKNOWN: &str = "\u{2014}";

/// A figure of the fee table: what is shown and what hovering it adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeFigure {
    pub text: String,
    pub title: Option<String>,
}

impl FeeFigure {
    fn unknown() -> Self {
        Self {
            text: UNKNOWN.to_owned(),
            title: None,
        }
    }

    /// A percentage: two decimals shown, four and the inputs on hover.
    fn percent(value: &str, inputs: &str) -> Self {
        let precise = round_half_up(value, 4);
        Self {
            text: format!("{}%", round_half_up(value, 2)),
            title: Some(if inputs.is_empty() {
                format!("{precise}%")
            } else {
                format!("{precise}% ({inputs})")
            }),
        }
    }
}

/// The fees of one character at one location, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeLocationView {
    pub name: String,
    pub sales_tax: FeeFigure,
    pub broker_fee: FeeFigure,
    pub min_margin: FeeFigure,
}

/// Rounds a decimal text half up (half away from zero for a negative one) to `decimals` places,
/// exactly: the text never goes through a float.
pub fn round_half_up(text: &str, decimals: usize) -> String {
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let mut int_len = integer.len();
    let mut digits: Vec<u8> = integer.bytes().map(|b| b - b'0').collect();
    digits.extend(fraction.bytes().take(decimals).map(|b| b - b'0'));
    digits.resize(int_len + decimals, 0);
    if fraction
        .as_bytes()
        .get(decimals)
        .is_some_and(|b| *b >= b'5')
    {
        let mut at = digits.len();
        loop {
            if at == 0 {
                digits.insert(0, 1);
                int_len += 1;
                break;
            }
            at -= 1;
            if digits[at] == 9 {
                digits[at] = 0;
            } else {
                digits[at] += 1;
                break;
            }
        }
    }
    let shown = |range: &[u8]| {
        range
            .iter()
            .map(|d| char::from(b'0' + d))
            .collect::<String>()
    };
    let mut out = shown(&digits[..int_len]);
    if decimals > 0 {
        out.push('.');
        out.push_str(&shown(&digits[int_len..]));
    }
    if negative && digits.iter().any(|d| *d != 0) {
        out.insert(0, '-');
    }
    out
}

fn location_view(row: &FeeRow, name: String) -> FeeLocationView {
    let sales_tax = match (&row.sales_tax, row.accounting) {
        (Some(tax), Some(level)) => FeeFigure::percent(tax, &format!("Accounting {level}")),
        _ => FeeFigure::unknown(),
    };
    let broker_fee = match (&row.broker_fee, row.broker_relations) {
        _ if row.is_structure => FeeFigure {
            text: "0.5% + owner fee".to_owned(),
            title: Some(
                "0.5% plus the fee the structure owner sets, which is not known".to_owned(),
            ),
        },
        (Some(fee), Some(level)) => {
            let standing = |value: &Option<String>| {
                value
                    .as_deref()
                    .map_or_else(|| "0.00".to_owned(), |v| round_half_up(v, 2))
            };
            FeeFigure::percent(
                fee,
                &format!(
                    "Broker Relations {level}, faction standing {}, corporation standing {}",
                    standing(&row.faction_standing),
                    standing(&row.corp_standing)
                ),
            )
        }
        _ => FeeFigure::unknown(),
    };
    let min_margin = match &row.min_margin {
        Some(margin) => FeeFigure::percent(margin, ""),
        None => FeeFigure::unknown(),
    };
    FeeLocationView {
        name,
        sales_tax,
        broker_fee,
        min_margin,
    }
}

/// The fees of one selected character at its fee location, ready for the fee cards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeCard {
    pub character_id: i64,
    pub character: String,
    /// The location scope is missing, so the location is Jita 4-4: the cards offer to log in again.
    pub needs_location_scope: bool,
    pub location: FeeLocationView,
}

/// The name of a location as the pages show it: the cached name, Jita 4-4 by its known name, a
/// structure that cannot be described, or the bare ID.
fn location_name(
    names: &HashMap<i64, Option<String>>,
    location_id: i64,
    is_structure: bool,
) -> String {
    match names.get(&location_id) {
        Some(Some(name)) => name.clone(),
        Some(None) if is_structure => format!("Unknown structure ({location_id})"),
        _ if location_id == STATION_ID => STATION_NAME.to_owned(),
        _ => format!("#{location_id}"),
    }
}

/// One card set per selected character of the account, in the order the characters were linked:
/// its fees at its fee location.
pub async fn fee_cards(
    stores: &Stores,
    account_id: Uuid,
    character_ids: &[i64],
) -> Result<Vec<FeeCard>, StoreError> {
    let fees = stores
        .fees
        .at_fee_location(account_id, character_ids)
        .await?;
    let location_ids: Vec<i64> = fees.iter().map(|f| f.row.location_id).collect();
    let names: HashMap<i64, Option<String>> = stores
        .market
        .names_of(&location_ids)
        .await?
        .into_iter()
        .collect();
    let characters: Vec<(i64, String)> = stores
        .accounts
        .list_characters(account_id)
        .await?
        .into_iter()
        .filter(|c| character_ids.contains(&c.character_id))
        .map(|c| (c.character_id, c.name))
        .collect();
    Ok(characters
        .into_iter()
        .filter_map(|(character_id, character)| {
            let fee = fees.iter().find(|f| f.row.character_id == character_id)?;
            let name = location_name(&names, fee.row.location_id, fee.row.is_structure);
            Some(FeeCard {
                character_id,
                character,
                needs_location_scope: fee.needs_location_scope,
                location: location_view(&fee.row, name),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_is_half_up_on_the_decimal_text() {
        for (text, decimals, expected) in [
            ("3.375", 2, "3.38"),
            ("3.374999", 2, "3.37"),
            ("6.50632", 4, "6.5063"),
            ("6.50635", 4, "6.5064"),
            ("1.410", 4, "1.4100"),
            ("1.41", 2, "1.41"),
            ("2.0", 2, "2.00"),
            ("0", 2, "0.00"),
            ("9.995", 2, "10.00"),
            ("99.5", 0, "100"),
            ("0.07999999821186066", 2, "0.08"),
            ("-5", 2, "-5.00"),
            ("-0.004", 2, "0.00"),
            ("-1.005", 2, "-1.01"),
        ] {
            assert_eq!(
                round_half_up(text, decimals),
                expected,
                "{text} at {decimals}"
            );
        }
    }

    fn row() -> FeeRow {
        FeeRow {
            character_id: 1,
            location_id: 60003760,
            is_structure: false,
            accounting: Some(5),
            broker_relations: Some(5),
            faction_standing: Some("2.0".to_owned()),
            corp_standing: Some("1.5".to_owned()),
            sales_tax: Some("3.375".to_owned()),
            broker_fee: Some("1.410".to_owned()),
            min_margin: Some("6.5063123".to_owned()),
        }
    }

    #[test]
    fn an_npc_station_shows_two_decimals_and_hovers_with_four_and_the_inputs() {
        let view = location_view(&row(), "Jita".to_owned());
        assert_eq!(view.sales_tax.text, "3.38%");
        assert_eq!(
            view.sales_tax.title.as_deref(),
            Some("3.3750% (Accounting 5)")
        );
        assert_eq!(view.broker_fee.text, "1.41%");
        assert_eq!(
            view.broker_fee.title.as_deref(),
            Some("1.4100% (Broker Relations 5, faction standing 2.00, corporation standing 1.50)")
        );
        assert_eq!(view.min_margin.text, "6.51%");
        assert_eq!(view.min_margin.title.as_deref(), Some("6.5063%"));
    }

    #[test]
    fn a_structure_reads_owner_fee_and_no_margin() {
        let structure = FeeRow {
            is_structure: true,
            broker_fee: None,
            min_margin: None,
            faction_standing: None,
            corp_standing: None,
            ..row()
        };
        let view = location_view(&structure, "Keepstar".to_owned());
        assert_eq!(view.broker_fee.text, "0.5% + owner fee");
        assert_eq!(view.min_margin.text, "\u{2014}");
        assert_eq!(view.sales_tax.text, "3.38%");
    }

    #[test]
    fn unknown_figures_read_a_dash_without_a_hover() {
        let nothing = FeeRow {
            accounting: None,
            broker_relations: None,
            faction_standing: None,
            corp_standing: None,
            sales_tax: None,
            broker_fee: None,
            min_margin: None,
            ..row()
        };
        let view = location_view(&nothing, "Jita".to_owned());
        for figure in [&view.sales_tax, &view.broker_fee, &view.min_margin] {
            assert_eq!(figure.text, "\u{2014}");
            assert_eq!(figure.title, None);
        }
    }
}
