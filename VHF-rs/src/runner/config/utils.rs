use crate::Result;
use configparser::ini;
use evalexpr::{DefaultNumericTypes, HashMapContext, Value};
use once_cell::sync::Lazy;
use regex::{Captures, Regex};

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
        log::debug!("[PythonMath::eval] called with self = {:?}", &self);
        let mut context = HashMapContext::<DefaultNumericTypes>::new();
        // Replace any 2**n Python expressions as evalexpr crate would coerce into float.
        let to_eval: Result<String> = if has_pow_2(&self) {
            // Check if u32 parse error occurs
            if HAS_POW_2_RE
                .captures_iter(&self)
                .any(|captured: Captures| captured["expo"].parse::<u32>().is_err())
            {
                return Err(crate::Error::ParseUnrecognised(
                    "Failed to parse exponent as u32".to_string(),
                ));
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
        log::debug!("[PythonMath::eval] evaluating on {:?}", &to_eval);
        let result = evalexpr::eval_with_context(&to_eval.unwrap(), &mut context)
            .map_err(crate::Error::EvalExpr)?;
        match result {
            Value::Boolean(_) => Err(crate::Error::IniParse(
                "Could not cast into numeric from Boolean".to_string(),
            )),
            Value::Empty => Err(crate::Error::IniParse(
                "Could not cast into numeric from Empty".to_string(),
            )),
            Value::Float(t) => Ok(Value::Float(t)),
            Value::Int(t) => Ok(Value::Int(t)),
            Value::String(_) => Err(crate::Error::IniParse(
                "Could not cast into numeric from String".to_string(),
            )),
            Value::Tuple(_) => Err(crate::Error::IniParse(
                "Could not cast into numeric from Tuple".to_string(),
            )),
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
        return Err(crate::Error::ini_missing(section, key));
    }
    log::debug!("ini file '{section} - {key_enable}' found.");
    // Check if key_enable is boolean
    if let None = config.getbool(section, key_enable).unwrap() {
        return Err(crate::Error::IniParse(
            "ini file '{section} - {key_enable}' was not boolean.".to_string(),
        ));
    }

    // We now check if key_enable is true/false
    if !config.getbool(section, key_enable).unwrap().unwrap() {
        // _enable key found to be false
        return Ok(None);
    }
    // key_enable found to be true, we now try to push to read the regular key.

    // Err(...) => key exists, parsing failed => TryWithParse or Error
    // Ok(None) => key ????, value not found => Error
    // Ok(Some(v)) => key exists, value parsed => bounds(v)
    if let Ok(None) = config.getuint(section, key) {
        log::warn!("ini file '{section} - {key}' not found.");
        return Err(crate::Error::ini_missing(section, key));
    }

    let v: u64 = match config.getuint(section, key) {
        Ok(None) => unreachable!(),
        Ok(Some(t)) => t,
        Err(_) => match config.get(section, key).unwrap().eval() {
            Err(e) => return Err(e),
            Ok(Value::Int(v)) => v as u64,
            Ok(t) => {
                log::trace!("{section} - {key} getuint yielded {}", t);
                return Err(crate::Error::IniParse(
                    "ini file '{section} - {key}' was not integer.".to_string(),
                ));
            }
        },
    };
    if !bound(v) {
        return Err(crate::Error::IniParse(
            "ini file '{section} - {key}' not within bounds.".to_string(),
        ));
    }

    Ok(Some(T::try_from(v).map_err(|_| {
        crate::Error::ParseUnrecognised(format!(
            "ini file '{section} - {key}' got value {v:?} but could not be coerced into T."
        ))
    })?))
}

// Account for the fact that ExtendedInterpolation is not provided by config
const EXT_INTERP_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\$\{((?<section>\S+):)?(?<key>\S+)\}").unwrap());

/// As [configparser] has yet to implement Basic/Extended Interpolation, we fetch a value and
/// interpolate before storing in the struct.
pub fn get_with_ext_interp(config: &ini::Ini, section: &str, key: &str) -> Result<String> {
    let read_value = config.get(section, key).map_or("".to_string(), |v| v);
    log::debug!("external_interpolate: key = {key}; read_value = {read_value}");
    if EXT_INTERP_RE.is_match(&read_value) {
        Ok(EXT_INTERP_RE
            .replace(&read_value, |capt: &Captures| -> String {
                // Index 1 is associated with section. If it fails, means there was no section, and
                // we use the current section
                // "section" and "key" keys in capt are given by EXT_INTERP_RE construct
                let section = capt.get(1).map_or(section, |_| &capt["section"]);
                let key = &capt["key"];
                get_with_ext_interp(config, section, key).unwrap_or("".to_owned())
            })
            .to_string())
    } else {
        Ok(read_value)
    }
}

#[cfg(test)]
mod pythonmath_tests {
    use super::PythonMath;
    use evalexpr::Value;
    use test_log::test;

    #[test]
    fn pythonmath_basic() {
        let inp = "27+ 32".to_string();
        let result: i64 = match inp.eval().unwrap() {
            Value::Int(t) => t,
            _ => panic!(),
        };
        let expected = 59;
        assert_eq!(result, expected)
    }

    #[test]
    fn pythonmath_2exponentiation() {
        let inp = "2**  27 + 1".to_string();
        let result: i64 = match inp.eval().unwrap() {
            Value::Int(t) => t,
            _ => panic!(),
        };
        let expected = (1 << 27) + 1;
        assert_eq!(result, expected)
    }
}

#[cfg(test)]
mod configutil_tests {
    use crate::Result;
    use configparser::ini;
    use test_log::test;

    #[test]
    fn value_enabled_false() {
        let mut conf = ini::Ini::new();
        let _ = match conf.read(String::from(
            "[Board]
             vga_num_enable = False",
        )) {
            Err(v) => panic!("{}", v),
            Ok(v) => v,
        };
        let result: Result<Option<u8>> =
            super::if_enabled_value(&conf, "Board", "vga_num", |v| v <= 8);
        let expected = None;
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn value_enabled_true_normal() {
        let mut conf = ini::Ini::new();
        let _ = match conf.read(String::from(
            "[Board]
             vga_num_enable = True
             vga_num = 2",
        )) {
            Err(v) => panic!("{}", v),
            Ok(v) => v,
        };
        let result: Result<Option<u8>> =
            super::if_enabled_value(&conf, "Board", "vga_num", |v| v <= 8);
        let expected = Some(2);
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn value_enabled_true_parse_error() {
        let mut conf = ini::Ini::new();
        let _ = match conf.read(String::from(
            "[Board]
             vga_num_enable = True
             vga_num = 2v",
        )) {
            Err(v) => panic!("{}", v),
            Ok(v) => v,
        };
        let result: Result<Option<u8>> =
            super::if_enabled_value(&conf, "Board", "vga_num", |v| v <= 8);
        assert!(result.is_err()); // Current failing because code is being permissive
    }

    #[test]
    fn value_enabled_true_parse_partial_error() {
        let mut conf = ini::Ini::new();
        let _ = match conf.read(String::from(
            "[Board]
             vga_num_enable = True
             vga_num = 2*3",
        )) {
            Err(v) => panic!("{}", v),
            Ok(v) => v,
        };
        let result: Result<Option<u8>> =
            super::if_enabled_value(&conf, "Board", "vga_num", |v| v <= 8);
        let expected = Some(6u8);
        log::info!("result = {:?}", result);
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn external_interpolate_same_section() {
        let mut conf = ini::Ini::new();
        let _ = match conf.read(String::from(
            "[Paths]
            base_dir: .
            board: ${base_dir}/vhf_board.softlink
            ",
        )) {
            Err(v) => panic!("{}", v),
            Ok(v) => v,
        };
        let result = super::get_with_ext_interp(&conf, "Paths", "board");
        let expected = "./vhf_board.softlink";
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn external_interpolate_diff_section() {
        let mut conf = ini::Ini::new();
        let _ = match conf.read(String::from(
            "[A]
            a: ${B:b}/a

            [B]
            b: 2
            ",
        )) {
            Err(v) => panic!("{}", v),
            Ok(v) => v,
        };
        let result = super::get_with_ext_interp(&conf, "A", "a");
        let expected = "2/a";
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn external_interpolate_diff_section_missing() {
        let mut conf = ini::Ini::new();
        let _ = match conf.read(String::from(
            "[A]
            a: ${B:b}/a
            ",
        )) {
            Err(v) => panic!("{}", v),
            Ok(v) => v,
        };
        let result = super::get_with_ext_interp(&conf, "A", "a");
        assert!(result.is_err());
    }
}
