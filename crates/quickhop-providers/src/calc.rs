//! Calculator and unit conversion with fend-core: `2+2*3`, `5 km to miles`, `20% of 80`,
//! `0x1f to decimal`.

use std::time::{Duration, Instant};

use quickhop_core::{Action, Cancel, Item, ItemId, Provider, ProviderError, Query};

/// Provider name.
pub const NAME: &str = "calc";

/// Longest an evaluation may take before it is abandoned.
pub const TIMEOUT: Duration = Duration::from_millis(500);

/// Evaluates queries that contain a digit.
#[derive(Debug, Default)]
pub struct CalcProvider;

impl CalcProvider {
    /// A calculator.
    pub fn new() -> Self {
        Self
    }
}

struct Interrupt<'a> {
    cancel: &'a Cancel,
    deadline: Instant,
}

impl fend_core::Interrupt for Interrupt<'_> {
    fn should_interrupt(&self) -> bool {
        self.cancel.is_cancelled() || Instant::now() >= self.deadline
    }
}

/// The value of `expression`, or `None` when it is not a calculation: no digit, a fend
/// error, an empty result, or a result equal to the input (a bare number).
pub fn evaluate(expression: &str, cancel: &Cancel) -> Option<String> {
    let expression = expression.trim();
    if !expression.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut context = fend_core::Context::new();
    let interrupt = Interrupt {
        cancel,
        deadline: Instant::now() + TIMEOUT,
    };
    let result = fend_core::evaluate_with_interrupt(expression, &mut context, &interrupt).ok()?;
    let value = result.get_main_result().trim();
    (!value.is_empty() && value != expression).then(|| value.to_owned())
}

impl Provider for CalcProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn items(&self, query: &Query, cancel: &Cancel) -> Result<Vec<Item>, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled {
                provider: NAME.into(),
            });
        }
        Ok(evaluate(query.text(), cancel)
            .map(|value| {
                Item::new(
                    ItemId::new(NAME, "result"),
                    value.clone(),
                    Action::Copy { text: value },
                )
                .with_subtitle(format!("{} =", query.text()))
            })
            .into_iter()
            .collect())
    }

    fn is_computed(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(text: &str) -> Option<String> {
        evaluate(text, &Cancel::new())
    }

    #[test]
    fn arithmetic_and_units() {
        assert_eq!(eval("2+2*3").as_deref(), Some("8"));
        assert_eq!(
            eval("5 km to miles").as_deref(),
            Some("approx. 3.1068559612 miles")
        );
        assert_eq!(eval("1 GiB to MiB").as_deref(), Some("1024 MiB"));
    }

    #[test]
    fn non_calculations_give_nothing() {
        assert_eq!(eval("firefox"), None);
        assert_eq!(eval("smile"), None);
        assert_eq!(eval("42"), None);
        assert_eq!(eval("2 +"), None);
        assert_eq!(eval(""), None);
    }

    #[test]
    fn cancelled_evaluation_gives_nothing() {
        let cancel = Cancel::new();
        cancel.cancel();
        assert_eq!(evaluate("2+2", &cancel), None);
    }
}
