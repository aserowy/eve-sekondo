//! The legend: one wording per short label of the opportunity tables and detail pages.

use sekondo::opportunities::{
    legend::{self, Term},
    tables::Table,
};

fn shorts(terms: &[Term]) -> Vec<&'static str> {
    terms.iter().map(|term| term.short()).collect()
}

#[test]
fn every_column_of_every_table_has_a_legend_entry() {
    for table in Table::ALL {
        let terms = legend::table_terms(table);
        assert_eq!(terms.len(), table.keys().len(), "{}", table.id());
        for (key, term) in table.keys().into_iter().zip(&terms) {
            assert_eq!(key.term(), *term);
            assert!(!term.name().is_empty(), "{}", term.short());
            assert!(!term.meaning().is_empty(), "{}", term.short());
        }
    }
}

#[test]
fn a_table_legend_follows_its_columns() {
    assert_eq!(
        shorts(&legend::table_terms(Table::Reversion)),
        [
            "Item", "B", "S", "Net", "m", "ISK/d", "%B", "RSI", "V7", "V30", "\u{03C3}", "T_max",
            "T_fill", "V_min", "n b/s",
        ]
    );
}

#[test]
fn the_detail_legend_follows_the_figure_groups() {
    assert_eq!(
        shorts(&legend::detail_terms()),
        [
            "b",
            "t",
            "B",
            "Best sell",
            "S",
            "c",
            "r",
            "Net",
            "m",
            "U",
            "V7",
            "V30",
            "\u{03C1}",
            "q_buy",
            "q_sell",
            "\u{03C3}",
            "T_fill",
            "T_max",
            "V_min",
            "ISK/d",
        ]
    );
}

#[test]
fn the_best_buy_is_named_as_not_a_suggested_price() {
    assert!(
        Term::BestBuy.name().contains("not a suggested price"),
        "{}",
        Term::BestBuy.name()
    );
}

#[test]
fn the_allowed_holding_time_has_its_formula() {
    assert_eq!(
        Term::TMax.formula(),
        Some("min(3, (m / 2\u{03C3})\u{00B2}) days")
    );
    assert_eq!(Term::Cost.formula(), Some("B \u{00D7} (1 + b)"));
    assert_eq!(Term::Margin.formula(), Some("(r \u{2212} c) / c"));
}

#[test]
fn the_item_entry_explains_the_own_order_marks() {
    let meaning = Term::Item.meaning();
    assert!(
        meaning.contains("own buy") && meaning.contains("own sell"),
        "{meaning}"
    );
}
