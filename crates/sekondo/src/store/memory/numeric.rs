//! PostgreSQL's `numeric`, for the arithmetic the SQL does on money and percentages: the scale of
//! a result and the digits of a division are the ones PostgreSQL gives, so the in-memory store
//! hands back the same text and the same rounding.

use std::{cmp::Ordering, fmt};

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

/// What PostgreSQL does not exceed in a result scale.
const MAX_DISPLAY_SCALE: u32 = 1000;
/// The significant digits a division aims for.
const MIN_SIG_DIGITS: i64 = 16;

#[derive(Debug, Clone)]
pub struct Numeric {
    unscaled: BigInt,
    scale: u32,
}

fn pow10(exp: u32) -> BigInt {
    BigInt::from(10).pow(exp)
}

impl Numeric {
    pub fn new(unscaled: impl Into<BigInt>, scale: u32) -> Self {
        Self {
            unscaled: unscaled.into(),
            scale,
        }
    }

    pub fn int(value: i64) -> Self {
        Self::new(value, 0)
    }

    /// An amount of cents as ISK with two decimals, the way `numeric(20,2)` holds it.
    pub fn cents(cents: impl Into<BigInt>) -> Self {
        Self::new(cents, 2)
    }

    pub fn zero() -> Self {
        Self::new(0, 0)
    }

    /// A decimal text such as `2.0`, `-1.50` or `7`.
    pub fn parse(text: &str) -> Option<Self> {
        let (negative, unsigned) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        if whole.is_empty() && fraction.is_empty() {
            return None;
        }
        if !whole
            .chars()
            .chain(fraction.chars())
            .all(|c| c.is_ascii_digit())
        {
            return None;
        }
        let digits = format!("{whole}{fraction}");
        let mut unscaled: BigInt = digits.parse().ok()?;
        if negative {
            unscaled = -unscaled;
        }
        Some(Self::new(unscaled, fraction.len() as u32))
    }

    /// `float8::numeric`: the 15 significant digits `%.15g` prints, without trailing zeros.
    pub fn from_f64(value: f64) -> Self {
        if value == 0.0 {
            return Self::zero();
        }
        let scientific = format!("{:.14e}", value.abs());
        let (mantissa, exponent) = scientific.split_once('e').expect("an exponent");
        let exponent: i64 = exponent.parse().expect("an integer exponent");
        let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
        let digits = digits.trim_end_matches('0');
        let digits = if digits.is_empty() { "0" } else { digits };
        let mut unscaled: BigInt = digits.parse().expect("digits");
        // value = 0.D × 10^(exponent + 1) with D the digits.
        let scale = digits.len() as i64 - 1 - exponent;
        let scale = if scale < 0 {
            unscaled *= pow10((-scale) as u32);
            0
        } else {
            scale as u32
        };
        if value < 0.0 {
            unscaled = -unscaled;
        }
        Self::new(unscaled, scale)
    }

    pub fn is_zero(&self) -> bool {
        self.unscaled.is_zero()
    }

    pub fn add(&self, other: &Self) -> Self {
        let scale = self.scale.max(other.scale);
        Self::new(self.at(scale) + other.at(scale), scale)
    }

    pub fn sub(&self, other: &Self) -> Self {
        let scale = self.scale.max(other.scale);
        Self::new(self.at(scale) - other.at(scale), scale)
    }

    pub fn mul(&self, other: &Self) -> Self {
        let scale = self.scale + other.scale;
        let product = &self.unscaled * &other.unscaled;
        if scale > MAX_DISPLAY_SCALE {
            Self::new(product, scale).round(MAX_DISPLAY_SCALE)
        } else {
            Self::new(product, scale)
        }
    }

    /// `self / other` with PostgreSQL's choice of the result scale. `None` for a division by zero.
    pub fn div(&self, other: &Self) -> Option<Self> {
        if other.is_zero() {
            return None;
        }
        let scale = select_div_scale(self, other);
        // The quotient at `scale` digits: u1 / 10^s1 / (u2 / 10^s2) × 10^scale.
        let numerator = &self.unscaled * pow10(other.scale + scale);
        let denominator = &other.unscaled * pow10(self.scale);
        Some(Self::new(round_div(&numerator, &denominator), scale))
    }

    /// Rounds half away from zero to `scale` digits; a larger scale only pads.
    pub fn round(&self, scale: u32) -> Self {
        if scale >= self.scale {
            return Self::new(self.at(scale), scale);
        }
        let divisor = pow10(self.scale - scale);
        Self::new(round_div(&self.unscaled, &divisor), scale)
    }

    /// The cast to `bigint`: rounds half away from zero.
    pub fn to_i64(&self) -> Option<i64> {
        self.round(0).unscaled.to_i64()
    }

    pub fn to_f64(&self) -> f64 {
        self.to_string()
            .parse()
            .expect("a decimal parses as a float")
    }

    fn at(&self, scale: u32) -> BigInt {
        &self.unscaled * pow10(scale - self.scale)
    }
}

/// `a / b` rounded half away from zero.
fn round_div(a: &BigInt, b: &BigInt) -> BigInt {
    let (quotient, remainder) = a.div_rem(b);
    if (remainder.abs() * 2u32) >= b.abs() {
        if (a.is_negative()) != (b.is_negative()) {
            quotient - BigInt::one()
        } else {
            quotient + BigInt::one()
        }
    } else {
        quotient
    }
}

/// The weight and first digit of the first non-zero base-10000 digit, as PostgreSQL keeps them.
fn weight_and_first_digit(value: &Numeric) -> (i64, i64) {
    if value.is_zero() {
        return (0, 0);
    }
    let magnitude = value.unscaled.abs();
    // Digits of the magnitude, then the position of the decimal point.
    let text = magnitude.to_string();
    let digits = text.len() as i64;
    // Decimal exponent of the leading digit: value = d.ddd × 10^exp10.
    let exp10 = digits - 1 - i64::from(value.scale);
    let weight = exp10.div_euclid(4);
    // The first base-10000 digit: the value divided by 10^(4 weight), without its fraction.
    let shift = 4 * weight + i64::from(value.scale);
    let first = if shift >= 0 {
        (&magnitude / pow10(shift as u32)).to_i64().unwrap_or(0)
    } else {
        (&magnitude * pow10((-shift) as u32)).to_i64().unwrap_or(0)
    };
    (weight, first)
}

/// PostgreSQL's `select_div_scale`: at least 16 significant digits, and never less than either
/// operand's scale.
fn select_div_scale(a: &Numeric, b: &Numeric) -> u32 {
    let (weight1, first1) = weight_and_first_digit(a);
    let (weight2, first2) = weight_and_first_digit(b);
    let mut qweight = weight1 - weight2;
    if first1 <= first2 {
        qweight -= 1;
    }
    let mut scale = MIN_SIG_DIGITS - qweight * 4;
    scale = scale.max(i64::from(a.scale)).max(i64::from(b.scale)).max(0);
    scale.min(i64::from(MAX_DISPLAY_SCALE)) as u32
}

impl PartialEq for Numeric {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Numeric {}

impl PartialOrd for Numeric {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Numeric {
    fn cmp(&self, other: &Self) -> Ordering {
        let scale = self.scale.max(other.scale);
        self.at(scale).cmp(&other.at(scale))
    }
}

/// The plain decimal text, with as many decimals as the scale says: `3.375`, `7.500`, `0`.
impl fmt::Display for Numeric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let negative = self.unscaled.is_negative();
        let digits = self.unscaled.abs().to_string();
        let scale = self.scale as usize;
        let padded = if digits.len() <= scale {
            format!("{}{digits}", "0".repeat(scale + 1 - digits.len()))
        } else {
            digits
        };
        let (whole, fraction) = padded.split_at(padded.len() - scale);
        if negative {
            f.write_str("-")?;
        }
        f.write_str(whole)?;
        if scale > 0 {
            write!(f, ".{fraction}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(text: &str) -> Numeric {
        Numeric::parse(text).unwrap()
    }

    // The expected texts below are what PostgreSQL 17 prints for the same expressions.

    #[test]
    fn a_text_keeps_its_scale() {
        assert_eq!(n("2.0").to_string(), "2.0");
        assert_eq!(n("7").to_string(), "7");
        assert_eq!(n("-0.50").to_string(), "-0.50");
        assert_eq!(n("0.001").to_string(), "0.001");
        assert!(Numeric::parse("1e5").is_none());
        assert!(Numeric::parse("").is_none());
    }

    #[test]
    fn addition_and_subtraction_keep_the_larger_scale_and_multiplication_adds_the_scales() {
        assert_eq!(n("1.5").add(&n("0.25")).to_string(), "1.75");
        assert_eq!(n("3").sub(&n("1.50")).to_string(), "1.50");
        // 7.5 * (1 - 0.11 * 5)
        let tax = n("7.5").mul(&Numeric::int(1).sub(&n("0.11").mul(&Numeric::int(5))));
        assert_eq!(tax.to_string(), "3.375");
        // 3 - 0.3 * 5 - 0.03 * 2.0 - 0.02 * 1.5
        let fee = Numeric::int(3)
            .sub(&n("0.3").mul(&Numeric::int(5)))
            .sub(&n("0.03").mul(&n("2.0")))
            .sub(&n("0.02").mul(&n("1.5")));
        assert_eq!(fee.to_string(), "1.410");
    }

    #[test]
    fn a_division_aims_for_sixteen_significant_digits() {
        assert_eq!(
            n("1.410").div(&Numeric::int(100)).unwrap().to_string(),
            "0.01410000000000000000"
        );
        assert_eq!(
            n("3.0").div(&Numeric::int(7)).unwrap().to_string(),
            "0.42857142857142857143"
        );
        assert_eq!(
            Numeric::int(1).div(&Numeric::int(3)).unwrap().to_string(),
            "0.33333333333333333333"
        );
        assert_eq!(
            n("10.50")
                .sub(&n("10.00"))
                .div(&n("10.00"))
                .unwrap()
                .to_string(),
            "0.05000000000000000000"
        );
        assert_eq!(
            Numeric::int(2)
                .div(&Numeric::int(1_000_000))
                .unwrap()
                .to_string(),
            "0.000002000000000000000000"
        );
        assert!(Numeric::int(1).div(&Numeric::zero()).is_none());
    }

    #[test]
    fn the_minimum_margin_formula_gives_postgres_digits() {
        // ((1 + b/100) / (1 - b/100 - t/100) - 1) * 100 with b = 1.410 and t = 3.375
        let hundred = Numeric::int(100);
        let b = n("1.410").div(&hundred).unwrap();
        let t = n("3.375").div(&hundred).unwrap();
        let margin = Numeric::int(1)
            .add(&b)
            .div(&Numeric::int(1).sub(&b).sub(&t))
            .unwrap()
            .sub(&Numeric::int(1))
            .mul(&hundred);
        assert_eq!(margin.to_string(), "6.50632778448773827700");
    }

    #[test]
    fn rounding_is_half_away_from_zero() {
        assert_eq!(n("2.345").round(2).to_string(), "2.35");
        assert_eq!(n("-2.345").round(2).to_string(), "-2.35");
        assert_eq!(n("2.344").round(2).to_string(), "2.34");
        assert_eq!(n("2.5").to_i64(), Some(3));
        assert_eq!(n("-2.5").to_i64(), Some(-3));
        assert_eq!(n("2.49").to_i64(), Some(2));
        assert_eq!(n("1.5").round(3).to_string(), "1.500");
    }

    #[test]
    fn a_float_becomes_its_fifteen_significant_digits() {
        assert_eq!(Numeric::from_f64(0.5).to_string(), "0.5");
        assert_eq!(
            Numeric::from_f64(0.079_999_998_211_860_66).to_string(),
            "0.0799999982118607"
        );
        assert_eq!(Numeric::from_f64(1e20).to_string(), "100000000000000000000");
        assert_eq!(
            Numeric::from_f64(123_456_789.123_456_79).to_string(),
            "123456789.123457"
        );
        assert_eq!(Numeric::from_f64(-0.05).to_string(), "-0.05");
        assert_eq!(Numeric::from_f64(0.0).to_string(), "0");
        assert_eq!(Numeric::from_f64(5_000_000.0).to_string(), "5000000");
    }

    #[test]
    fn values_compare_by_value_not_by_scale() {
        assert_eq!(n("1.50"), n("1.5"));
        assert!(n("1.49") < n("1.5"));
        assert!(n("-1") < n("0"));
    }

    #[test]
    fn a_numeric_converts_to_a_float_through_its_text() {
        assert_eq!(n("0.0799999982118607").to_f64(), 0.0799999982118607);
        assert_eq!(n("6.50632778448773827700").to_f64(), 6.5063277844877385);
    }
}
