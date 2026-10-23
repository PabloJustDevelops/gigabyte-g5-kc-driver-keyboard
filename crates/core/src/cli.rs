//! A small argument splitter, shared by the two CLIs.
//!
//! Not a parser framework: the CLIs here have a handful of subcommands each and
//! their flags are known at compile time. What matters is that *both* of them
//! treat an argument the same way — flags in any position, `--opt value` and
//! `--opt=value`, `--` to end option parsing — and that the behaviour is
//! testable without spawning a process.
//!
//! Why hand-rolled rather than `clap`: `clap` is not a dependency of this
//! workspace today, and the two CLIs must stay buildable with the vendored
//! registry cache (`cargo --frozen`), which a new dependency would invalidate.

use std::collections::{BTreeMap, BTreeSet};

/// One option a CLI accepts.
#[derive(Clone, Copy, Debug)]
pub struct Opt {
    name: &'static str,
    aliases: &'static [&'static str],
    takes_value: bool,
}

impl Opt {
    /// `--json`: present or not.
    pub const fn switch(name: &'static str) -> Self {
        Self {
            name,
            aliases: &[],
            takes_value: false,
        }
    }

    /// `--speed 5`.
    pub const fn value(name: &'static str) -> Self {
        Self {
            name,
            aliases: &[],
            takes_value: true,
        }
    }

    /// A switch that also answers to a short form, e.g. `--bg`/`-b`.
    pub const fn switch_also(name: &'static str, aliases: &'static [&'static str]) -> Self {
        Self {
            name,
            aliases,
            takes_value: false,
        }
    }

    /// A value option that also answers to a short form, e.g. `--brightness`/`-b`.
    pub const fn value_also(name: &'static str, aliases: &'static [&'static str]) -> Self {
        Self {
            name,
            aliases,
            takes_value: true,
        }
    }
}

/// Positionals, switches and options, split out of raw `argv`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    positional: Vec<String>,
    switches: BTreeSet<String>,
    options: BTreeMap<String, String>,
}

impl Args {
    /// Split `words` into positionals, switches and options.
    ///
    /// Anything starting with `-` that is not one of `opts` is an error rather
    /// than a positional: a typo like `--wirte` should be complained about, not
    /// silently treated as a filename.
    pub fn parse(words: &[String], opts: &[Opt]) -> Result<Self, String> {
        let mut args = Self::default();
        let mut iter = words.iter();
        let mut literal = false;
        while let Some(word) = iter.next() {
            if literal {
                args.positional.push(word.clone());
                continue;
            }
            if word == "--" {
                literal = true;
                continue;
            }
            if !word.starts_with('-') || word == "-" {
                args.positional.push(word.clone());
                continue;
            }
            let (token, inline) = match word.split_once('=') {
                Some((token, value)) => (token, Some(value)),
                None => (word.as_str(), None),
            };
            let Some(opt) = resolve(opts, token) else {
                return Err(format!("unrecognized argument: {word}"));
            };
            if opt.takes_value {
                let value = match inline {
                    Some(value) => value.to_string(),
                    None => iter
                        .next()
                        .ok_or_else(|| format!("{token} needs a value"))?
                        .clone(),
                };
                args.options.insert(opt.name.to_string(), value);
            } else if inline.is_some() {
                return Err(format!("{token} does not take a value"));
            } else {
                args.switches.insert(opt.name.to_string());
            }
        }
        Ok(args)
    }

    /// Everything that was not an option, in order: `["curve", "set", "50:25"]`.
    pub fn positional(&self) -> &[String] {
        &self.positional
    }

    /// The first positional, which is the subcommand.
    pub fn command(&self) -> Option<&str> {
        self.positional.first().map(String::as_str)
    }

    /// The positionals after the subcommand.
    pub fn rest(&self) -> &[String] {
        self.positional.get(1..).unwrap_or_default()
    }

    pub fn has(&self, switch: &str) -> bool {
        self.switches.contains(switch)
    }

    pub fn value(&self, opt: &str) -> Option<&str> {
        self.options.get(opt).map(String::as_str)
    }

    pub fn value_u8(&self, opt: &str) -> Result<Option<u8>, String> {
        match self.value(opt) {
            None => Ok(None),
            Some(text) => text
                .parse::<u8>()
                .map(Some)
                .map_err(|_| format!("--{opt} must be a number 0..255 (got '{text}')")),
        }
    }

    pub fn value_i32(&self, opt: &str) -> Result<Option<i32>, String> {
        match self.value(opt) {
            None => Ok(None),
            Some(text) => text
                .parse::<i32>()
                .map(Some)
                .map_err(|_| format!("--{opt} must be a whole number (got '{text}')")),
        }
    }

    pub fn value_f64(&self, opt: &str) -> Result<Option<f64>, String> {
        match self.value(opt) {
            None => Ok(None),
            Some(text) => text
                .parse::<f64>()
                .map(Some)
                .map_err(|_| format!("--{opt} must be a number (got '{text}')")),
        }
    }
}

/// Match a token like `--json` or `-c` against the declared options. Aliases
/// are written the way they are typed on the command line (`&["-c"]`), so the
/// leading dashes come off both sides of the comparison.
fn resolve<'a>(opts: &'a [Opt], token: &str) -> Option<&'a Opt> {
    let name = token.trim_start_matches('-');
    if name.is_empty() {
        return None;
    }
    opts.iter().find(|opt| {
        opt.name == name
            || opt
                .aliases
                .iter()
                .any(|alias| alias.trim_start_matches('-') == name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    /// `&[String]` compared as `&str`, so the expectations below read like the
    /// command line they came from.
    fn seen(args: &Args) -> Vec<&str> {
        args.positional().iter().map(String::as_str).collect()
    }

    const OPTS: [Opt; 4] = [
        Opt::switch("json"),
        Opt::switch_also("bg", &["-b"]),
        Opt::value("speed"),
        Opt::value_also("color", &["-c"]),
    ];

    #[test]
    fn positionals_keep_their_order() {
        let args = Args::parse(&words("curve set 50:25 65:40"), &OPTS).expect("parse");
        assert_eq!(args.command(), Some("curve"));
        assert_eq!(seen(&args), ["curve", "set", "50:25", "65:40"]);
        assert_eq!(args.rest().len(), 3);
    }

    #[test]
    fn flags_are_found_in_any_position() {
        let args = Args::parse(&words("--speed 8 effect cycle"), &OPTS).expect("parse");
        assert_eq!(args.value("speed"), Some("8"));
        assert_eq!(args.command(), Some("effect"));
        assert_eq!(seen(&args), ["effect", "cycle"]);

        let args = Args::parse(&words("effect cycle --speed 8"), &OPTS).expect("parse");
        assert_eq!(args.value("speed"), Some("8"));
        assert_eq!(seen(&args), ["effect", "cycle"]);
    }

    #[test]
    fn options_accept_an_equals_sign() {
        let args = Args::parse(&words("effect cycle --speed=3"), &OPTS).expect("parse");
        assert_eq!(args.value("speed"), Some("3"));
    }

    #[test]
    fn short_aliases_work() {
        let args = Args::parse(&words("effect breathe -c ff0066 -b"), &OPTS).expect("parse");
        assert_eq!(args.value("color"), Some("ff0066"));
        assert!(args.has("bg"));
    }

    #[test]
    fn switches_are_boolean() {
        let args = Args::parse(&words("status --json --json"), &OPTS).expect("parse");
        assert!(args.has("json"));
        assert!(!args.has("stale"));
    }

    #[test]
    fn a_missing_value_is_an_error_not_an_empty_string() {
        let error = Args::parse(&words("effect cycle --speed"), &OPTS).expect_err("refused");
        assert_eq!(error, "--speed needs a value");
    }

    #[test]
    fn a_typo_is_refused_instead_of_becoming_a_positional() {
        let error = Args::parse(&words("status --cachedd"), &OPTS).expect_err("refused");
        assert_eq!(error, "unrecognized argument: --cachedd");
    }

    #[test]
    fn a_switch_with_a_value_is_refused() {
        let error = Args::parse(&words("status --json=1"), &OPTS).expect_err("refused");
        assert_eq!(error, "--json does not take a value");
    }

    #[test]
    fn a_double_dash_ends_the_options() {
        let args = Args::parse(&words("fan -- --json"), &OPTS).expect("parse");
        assert_eq!(seen(&args), ["fan", "--json"]);
        assert!(!args.has("json"));
    }

    #[test]
    fn a_lone_dash_is_a_positional() {
        let args = Args::parse(&words("color -"), &OPTS).expect("parse");
        assert_eq!(seen(&args), ["color", "-"]);
    }

    #[test]
    fn numbers_are_validated_where_they_are_read() {
        let args = Args::parse(&words("effect cycle --speed nine"), &OPTS).expect("parse");
        assert!(args.value_i32("speed").is_err());
        let args = Args::parse(&words("effect cycle --speed 9"), &OPTS).expect("parse");
        assert_eq!(args.value_i32("speed"), Ok(Some(9)));
        assert_eq!(args.value_u8("speed"), Ok(Some(9)));
        assert!(args.value_u8("missing_opt").is_ok());
    }
}
