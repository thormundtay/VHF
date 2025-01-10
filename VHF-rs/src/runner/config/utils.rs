use crate::Result;
use configparser::ini;
use evalexpr::{DefaultNumericTypes, HashMapContext, Value};
use once_cell::sync::Lazy;
use regex::{Captures, Regex};

impl From<std::convert::Infallible> for crate::Error {
    fn from(value: std::convert::Infallible) -> Self {
        value.try_into().unwrap()
    }
}

/// Used to run `eval` on Python strings, specific to getting only Math types
pub trait PythonMath {
    /// Subset of Python's eval function to work with math expressions.
    fn eval(self) -> Result<evalexpr::Value>;
}

/// Regex to check for 2 followed by exponent with arbitrary whitespace.
const HAS_POW_2_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"2{1}\s*\*\*\s*(?<expo>\d+)\s*").unwrap());
/// check if `s` contains any form of `2 ** ` expression.
fn has_pow_2(s: &str) -> bool {
    HAS_POW_2_RE.is_match(s)
}

impl PythonMath for String {
    fn eval(self) -> Result<evalexpr::Value> {
        let mut context = HashMapContext::<DefaultNumericTypes>::new();
        // Replace any 2**n Python expressions as evalexpr crate would coerce into float.
        let to_eval: Result<String> = if has_pow_2(&self) {
            // Check if u32 parse error occurs
            if HAS_POW_2_RE
                .captures_iter(&self)
                .any(|captured: Captures| captured["expo"].parse::<u32>().is_err())
            {
                return Err(crate::Error::new("Failed to parse exponent as u32"));
            }

            // No parse int error found, we can continue
            let tmp = HAS_POW_2_RE
                .replace(&self, |captured: &Captures| {
                    // log::trace!("closure: captured = {:?}", captured);
                    let exponent = captured["expo"].parse::<u32>().unwrap();
                    format!("{}", 1 << exponent)
                })
                .to_string();
            log::trace!("Expression to evaluate: {}", tmp);
            Ok(tmp)
        } else {
            Ok(self)
        };
        // We let evalexpr handle everything except for powers of 2
        let result = evalexpr::eval_with_context(&to_eval.unwrap(), &mut context).unwrap();
        match result {
            Value::Boolean(_) => Err(crate::Error::new(
                "Could not cast into generic from Boolean",
            )),
            Value::Empty => Err(crate::Error::new("Could not cast into generic from Empty")),
            Value::Float(t) => Ok(Value::Float(t)),
            Value::Int(t) => Ok(Value::Int(t)),
            Value::String(_) => Err(crate::Error::new("Could not cast into generic from String")),
            Value::Tuple(_) => Err(crate::Error::new("Could not cast into generic from Tuple")),
        }
    }
}

/// In the case of VHF_board_ini having "toggleable" keys, we check if the enable_key exists, then
/// the corresponding value of the actual key.
/// Returns Err if _enable could not be found,
///         Ok(None) if _enable key is false, Ok(val as T) otherwise.
///         if as T fails, returns Err
pub fn if_enabled_value<T>(
    config: &ini::Ini,
    section: &str,
    key: &str,
    bound: impl FnOnce(u64) -> bool,
) -> Result<Option<T>>
where
    T: num_traits::bounds::Bounded + Into<u64> + TryFrom<u64>,
{
    let key_enable = &format!("{}_enable", key);
    let map = config.get_map_ref();

    if let None = map
        .get(&section.to_ascii_lowercase())
        .expect(&format!("ini file '{}' section not found.", section))
        .get(key_enable)
    {
        return Err(crate::Error::new(&format!(
            "ini file '{section} - {key_enable}' not found."
        )));
    }
    log::debug!("ini file '{section} - {key_enable}' found.");
    // Check if key_enable is boolean
    if let None = config.getbool(section, key_enable).unwrap() {
        return Err(crate::Error::new(&format!(
            "ini file '{section} - {key_enable}' was not boolean."
        )));
    }

    // We now check if key_enable is true/false
    if !config.getbool(section, key_enable).unwrap().unwrap() {
        // _enable key found to be false
        return Ok(None);
    }
    // key_enable found to be true, we now try to push to read the regular key.

    // Err(...) => key exists, parsing failed => Error
    // Ok(None) => key ????, value not found => Error
    // Ok(Some(v)) => key exists, value parsed => bounds(v)
    if let Ok(None) = config.getuint(section, key) {
        log::warn!("ini file '{section} - {key}' not found.");
        return Err(crate::Error::new(&format!(
            "ini file '{section} - {key}' not found."
        )));
    }
    let v = config.getuint(section, key).unwrap_or_else(|v| {
        // Avoid eager evaluation creating logging side effect
        log::warn!("ini file '{section} - {key}' could not be parsed into u64 directly.");
        Some(v.parse().unwrap_or(0))
    });
    if !bound(v.unwrap()) {
        return Err(crate::Error::new(&format!(
            "ini file '{section} - {key}' not within bounds."
        )));
    }

    match T::try_from(v.unwrap()) {
        Ok(t) => Ok(Some(t)),
        Err(_) => Err(crate::Error::new(&format!("ini file '{section} - {key}' within bounds but not type-bounds (could not be coerced).")))
    }
}

#[cfg(test)]
mod tests {
    use test_log::test;
    // use tracing::info;

    use super::PythonMath;
    use evalexpr::Value;

    #[test]
    fn case_a() {
        let inp = "27+ 32".to_string();
        let result: i64 = match inp.eval().unwrap() {
            Value::Int(t) => t,
            _ => panic!(),
        };
        let expected = 59;
        assert_eq!(result, expected)
    }

    #[test]
    fn case_b() {
        let inp = "2**  27 + 1".to_string();
        let result: i64 = match inp.eval().unwrap() {
            Value::Int(t) => t,
            _ => panic!(),
        };
        let expected = (1 << 27) + 1;
        assert_eq!(result, expected)
    }
}
