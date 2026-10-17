//! What the market charges a character at each station or structure where it has orders: the
//! sales tax, the broker fee and the minimum margin of a station trade. One SQL statement works
//! them out, and both the orders page and the `fees-updated` change detection read it, so a
//! signal goes out exactly when what is shown changes (design §5).
//!
//! The values are `numeric` in the database and stay decimal text here: the crate has no decimal
//! type, and `f64` arithmetic would not be exact.

use std::collections::HashMap;

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

/// ESI's skill IDs of the two skills that matter.
pub const ACCOUNTING: i64 = 16622;
pub const BROKER_RELATIONS: i64 = 3446;

/// One row per (character, location) of the current orders. `$1` is the character IDs and `$2`
/// the account the characters must belong to (`NULL`: any).
///
/// - `accounting` and `broker_relations` are `NULL` until the skills were polled once, and 0 when
///   polled but not trained.
/// - `faction_standing` and `corp_standing` are `NULL` for a structure, until the standings were
///   polled once and until the station's owner is known. They are 0 when polled and absent, and
///   the faction standing is 0 when the owner is in no faction.
/// - `sales_tax`: 7.5 × (1 − 0.11 × accounting). `broker_fee` (NPC station only):
///   3 − 0.3·broker relations − 0.03·faction standing − 0.02·corporation standing.
///   `min_margin`: ((1 + b) / (1 − b − t) − 1) × 100 for a known broker fee b and sales tax t.
///   All in percent, and not clamped: the formula cannot fall below 1.0%.
pub const FEES_SQL: &str = "
    SELECT f.character_id, f.location_id, f.is_structure,
           f.accounting, f.broker_relations,
           f.faction_standing::text AS faction_standing, f.corp_standing::text AS corp_standing,
           f.sales_tax::text AS sales_tax, f.broker_fee::text AS broker_fee,
           CASE WHEN f.broker_fee IS NOT NULL AND f.sales_tax IS NOT NULL THEN
               (((1 + f.broker_fee / 100) / (1 - f.broker_fee / 100 - f.sales_tax / 100) - 1) * 100)::text
           END AS min_margin
    FROM (
        SELECT l.character_id, l.location_id, l.is_structure,
               l.accounting, l.broker_relations, l.faction_standing, l.corp_standing,
               CASE WHEN l.accounting IS NOT NULL THEN 7.5 * (1 - 0.11 * l.accounting) END AS sales_tax,
               CASE WHEN NOT l.is_structure
                         AND l.broker_relations IS NOT NULL
                         AND l.faction_standing IS NOT NULL
                         AND l.corp_standing IS NOT NULL
                    THEN 3 - 0.3 * l.broker_relations - 0.03 * l.faction_standing
                           - 0.02 * l.corp_standing
               END AS broker_fee
        FROM (
            SELECT loc.character_id, loc.location_id,
                   (loc.location_id >= 100000000) AS is_structure,
                   CASE WHEN skills_sync.last_success_at IS NOT NULL
                        THEN COALESCE(acc.active_level, 0)::integer END AS accounting,
                   CASE WHEN skills_sync.last_success_at IS NOT NULL
                        THEN COALESCE(br.active_level, 0)::integer END AS broker_relations,
                   CASE WHEN loc.location_id < 100000000
                             AND standings_sync.last_success_at IS NOT NULL
                             AND so.owner_id IS NOT NULL
                        THEN CASE WHEN nf.faction_id IS NULL THEN 0
                                  ELSE COALESCE(fs.standing, 0) END
                   END AS faction_standing,
                   CASE WHEN loc.location_id < 100000000
                             AND standings_sync.last_success_at IS NOT NULL
                             AND so.owner_id IS NOT NULL
                        THEN COALESCE(cs.standing, 0)
                   END AS corp_standing
            FROM (SELECT DISTINCT o.character_id, o.location_id
                  FROM current_orders o
                  JOIN characters c ON c.character_id = o.character_id
                  WHERE o.character_id = ANY($1) AND ($2::uuid IS NULL OR c.account_id = $2)) loc
            LEFT JOIN fee_sync skills_sync
                   ON skills_sync.character_id = loc.character_id AND skills_sync.kind = 'skills'
            LEFT JOIN fee_sync standings_sync
                   ON standings_sync.character_id = loc.character_id AND standings_sync.kind = 'standings'
            LEFT JOIN character_skills acc
                   ON acc.character_id = loc.character_id AND acc.skill_id = 16622
            LEFT JOIN character_skills br
                   ON br.character_id = loc.character_id AND br.skill_id = 3446
            LEFT JOIN station_owners so ON so.station_id = loc.location_id
            LEFT JOIN npc_corporation_factions nf ON nf.corporation_id = so.owner_id
            LEFT JOIN character_standings fs
                   ON fs.character_id = loc.character_id AND fs.from_type = 'faction'
                  AND fs.from_id = nf.faction_id
            LEFT JOIN character_standings cs
                   ON cs.character_id = loc.character_id AND cs.from_type = 'npc_corp'
                  AND cs.from_id = so.owner_id
        ) l
    ) f
    ORDER BY f.character_id, f.location_id";

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

/// The rows of these characters, whoever owns them. For the pollers, which work per character.
pub async fn for_characters<'e>(
    executor: impl PgExecutor<'e>,
    character_ids: &[i64],
) -> Result<Vec<FeeRow>, sqlx::Error> {
    sqlx::query_as(FEES_SQL)
        .bind(character_ids)
        .bind(None::<Uuid>)
        .fetch_all(executor)
        .await
}

/// The rows of these characters that belong to the account, so a selection can never reach
/// another account's data.
pub async fn for_account(
    pool: &PgPool,
    account_id: Uuid,
    character_ids: &[i64],
) -> Result<Vec<FeeRow>, sqlx::Error> {
    sqlx::query_as(FEES_SQL)
        .bind(character_ids)
        .bind(account_id)
        .fetch_all(pool)
        .await
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

/// The fee table of every selected character of the account, by character: one row per location
/// with orders, ordered by location name.
pub async fn locations_by_character(
    pool: &PgPool,
    account_id: Uuid,
    character_ids: &[i64],
) -> Result<HashMap<i64, Vec<FeeLocationView>>, sqlx::Error> {
    let rows = for_account(pool, account_id, character_ids).await?;
    let location_ids: Vec<i64> = rows.iter().map(|r| r.location_id).collect();
    let names: HashMap<i64, Option<String>> =
        sqlx::query_as::<_, (i64, Option<String>)>("SELECT id, name FROM names WHERE id = ANY($1)")
            .bind(&location_ids)
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect();
    let mut by_character: HashMap<i64, Vec<(String, i64, FeeLocationView)>> = HashMap::new();
    for row in &rows {
        let name = match names.get(&row.location_id) {
            Some(Some(name)) => name.clone(),
            Some(None) if row.is_structure => format!("Unknown structure ({})", row.location_id),
            _ => format!("#{}", row.location_id),
        };
        by_character.entry(row.character_id).or_default().push((
            name.to_lowercase(),
            row.location_id,
            location_view(row, name),
        ));
    }
    Ok(by_character
        .into_iter()
        .map(|(character, mut locations)| {
            locations.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
            (
                character,
                locations.into_iter().map(|(_, _, view)| view).collect(),
            )
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
