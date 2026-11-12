//! The small argument parser every command shares: positional arguments, `--flag <value>` (or
//! `--flag=<value>`) for the flags a command names, and everything after `--` kept as is.

use std::collections::BTreeMap;
use uncharted_core::games::Game;

/// One command's arguments, split.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub positional: Vec<String>,
    pub flags: BTreeMap<String, String>,
    /// The arguments after `--`, untouched.
    pub rest: Vec<String>,
}

impl Parsed {
    pub fn flag(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }
}

/// Split `args`. `flags` are the flags that take a value (without `--`); at most `positional`
/// arguments are allowed. Anything else is the problem, as a sentence.
pub fn parse(args: &[String], flags: &[&str], positional: usize) -> Result<Parsed, String> {
    let mut parsed = Parsed::default();
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        if arg == "--" {
            parsed.rest = items.by_ref().cloned().collect();
            break;
        }
        if let Some(flag) = arg.strip_prefix("--") {
            let (name, inline) = match flag.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (flag, None),
            };
            if !flags.contains(&name) {
                return Err(format!("unknown option '--{name}'"));
            }
            let value = match inline {
                Some(value) => value,
                None => items
                    .next()
                    .filter(|value| !value.starts_with("--"))
                    .cloned()
                    .ok_or_else(|| format!("--{name} needs a value"))?,
            };
            if value.is_empty() {
                return Err(format!("--{name} needs a value"));
            }
            if parsed.flags.insert(name.to_owned(), value).is_some() {
                return Err(format!("--{name} is given twice"));
            }
        } else if arg.len() > 1 && arg.starts_with('-') {
            return Err(format!("unknown option '{arg}'"));
        } else {
            parsed.positional.push(arg.clone());
        }
    }
    if parsed.positional.len() > positional {
        return Err(format!(
            "unexpected argument '{}'",
            parsed.positional[positional]
        ));
    }
    Ok(parsed)
}

/// A game key ("u1", "u2", "u3", any case), or the problem.
pub fn game(key: &str) -> Result<Game, String> {
    Game::from_key(key).ok_or_else(|| format!("unknown game '{key}' (use u1, u2 or u3)"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::args;

    #[test]
    fn splits_positional_flags_and_rest() {
        let parsed = parse(
            &args(&["u2", "--dump", "/d", "--keep=local", "--", "--x", "y"]),
            &["dump", "keep"],
            1,
        )
        .unwrap();
        assert_eq!(parsed.positional, ["u2"]);
        assert_eq!(parsed.flag("dump"), Some("/d"));
        assert_eq!(parsed.flag("keep"), Some("local"));
        assert_eq!(parsed.rest, ["--x", "y"]);
    }

    #[test]
    fn refuses_unknown_missing_repeated_and_extra() {
        let refuse = |list: &[&str]| parse(&args(list), &["dump"], 1).unwrap_err();
        assert_eq!(refuse(&["--frob"]), "unknown option '--frob'");
        assert_eq!(refuse(&["-x"]), "unknown option '-x'");
        assert_eq!(refuse(&["--dump"]), "--dump needs a value");
        assert_eq!(refuse(&["--dump", "--dump"]), "--dump needs a value");
        assert_eq!(refuse(&["--dump="]), "--dump needs a value");
        assert_eq!(
            refuse(&["--dump", "a", "--dump", "b"]),
            "--dump is given twice"
        );
        assert_eq!(refuse(&["a", "b"]), "unexpected argument 'b'");
    }

    #[test]
    fn reads_game_keys_in_any_case() {
        assert_eq!(game("U3"), Ok(Game::U3));
        assert_eq!(
            game("u4").unwrap_err(),
            "unknown game 'u4' (use u1, u2 or u3)"
        );
    }
}
