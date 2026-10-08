//! Exact decimal comparison without expanding exponents or converting to floats.
//! Explicit exponents are bounded by the workflow JSON byte budget in either direction.

use serde_json::Number;

const MAX_EXPONENT: i32 = super::MAX_WORKFLOW_JSON_BYTES as i32;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Decimal {
    negative: bool,
    coefficient: String,
    power: i32,
}

impl Decimal {
    pub(super) fn from_number(number: &Number) -> Result<Self, String> {
        let text = number.as_str();
        if text.len() > super::MAX_WORKFLOW_JSON_BYTES {
            return Err("workflow number exceeds byte budget".into());
        }
        let (mantissa, exponent) = match text.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => {
                let exponent = exponent
                    .parse::<i32>()
                    .map_err(|_| "workflow number exceeds exponent budget")?;
                if !(-MAX_EXPONENT..=MAX_EXPONENT).contains(&exponent) {
                    return Err("workflow number exceeds exponent budget".into());
                }
                (mantissa, exponent)
            }
            None => (text, 0),
        };
        let negative = mantissa.starts_with('-');
        let mantissa = mantissa.strip_prefix('-').unwrap_or(mantissa);
        let fractional_digits = mantissa
            .split_once('.')
            .map_or(0, |(_, fraction)| fraction.len());
        let digits: String = mantissa.chars().filter(|digit| *digit != '.').collect();
        let significant = digits.trim_start_matches('0');
        if significant.is_empty() {
            return Ok(Self {
                negative: false,
                coefficient: "0".into(),
                power: 0,
            });
        }
        let coefficient = significant.trim_end_matches('0');
        let trailing_zeros = significant.len() - coefficient.len();
        // Both lengths are bounded by MAX_WORKFLOW_JSON_BYTES; no exponent expansion.
        let power = exponent - fractional_digits as i32 + trailing_zeros as i32;
        Ok(Self {
            negative,
            coefficient: coefficient.into(),
            power,
        })
    }

    pub(super) fn is_integer(&self) -> bool {
        self.power >= 0
    }
}

#[cfg(test)]
#[path = "workflow_numeric_tests.rs"]
mod tests;
