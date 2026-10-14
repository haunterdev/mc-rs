//! `.lang` translation tables (`en_us.lang` from the client jar) and the formatting
//! `I18n.format` does with them.
//!
//! Parsing is `Locale.loadLocaleData` (and the identical `LanguageMap.<init>`): one entry per
//! line, lines that are empty or start with `#` skipped, the key before the first `=`, and every
//! `%d`/`%f` style argument in the value rewritten to `%s` (`PATTERN`,
//! `%(\d+\$)?[\d\.]*[df]` to `%$1s`). Formatting is `Locale.formatMessage`: `String.format` over
//! the value, `"Format error: "` plus the value when it throws. [`format`] covers the part of
//! `java.util.Formatter` vanilla lang values use: `%s`, `%S`, `%d`, `%%`, `%n`, positional
//! `%1$s`, width and the `-` flag.

use std::collections::HashMap;
use std::fmt;

/// A loaded translation table.
#[derive(Clone, Debug, Default)]
pub struct Lang {
    map: HashMap<String, String>,
}

impl Lang {
    /// `Locale.loadLocaleData(InputStream)` over a file's bytes, read as UTF-8.
    pub fn from_bytes(bytes: &[u8]) -> Lang {
        Lang::parse(&String::from_utf8_lossy(bytes))
    }

    /// `Locale.loadLocaleData(InputStream)`. Lines end at `\n`, `\r\n` or `\r`, as
    /// `BufferedReader.readLine` splits them; a later key replaces an earlier one.
    pub fn parse(text: &str) -> Lang {
        let mut map = HashMap::new();
        for line in text.split('\n').flat_map(|l| l.split('\r')) {
            // SPLITTER (`Splitter.on('=').limit(2)`) gives two parts only when there is an `=`.
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.to_string(), rewrite_numeric(v));
            }
        }
        Lang { map }
    }

    /// `Locale.translateKeyPrivate`: the value, or the key itself when the table lacks it.
    pub fn translate<'a>(&'a self, key: &'a str) -> &'a str {
        self.map.get(key).map_or(key, |s| s.as_str())
    }

    /// `Locale.hasKey`.
    pub fn has_key(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// `Locale.formatMessage` (`I18n.format`).
    pub fn format(&self, key: &str, args: &[Arg]) -> String {
        let s = self.translate(key);
        format(s, args).unwrap_or_else(|| format!("Format error: {s}"))
    }

    /// The table itself, for `items::names::display_name`.
    pub fn map(&self) -> &HashMap<String, String> {
        &self.map
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// A format argument: what `I18n.format` callers pass is strings and integers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    Str(String),
    Int(i64),
}

impl fmt::Display for Arg {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Arg::Str(s) => f.write_str(s),
            Arg::Int(n) => write!(f, "{n}"),
        }
    }
}

impl From<&str> for Arg {
    fn from(s: &str) -> Arg {
        Arg::Str(s.to_string())
    }
}

impl From<String> for Arg {
    fn from(s: String) -> Arg {
        Arg::Str(s)
    }
}

macro_rules! int_arg {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg {
            fn from(n: $t) -> Arg {
                Arg::Int(n as i64)
            }
        }
    )*};
}
int_arg!(i8, i16, i32, i64, u8, u16, u32);

/// `Locale.PATTERN.matcher(value).replaceAll("%$1s")`: `%(\d+\$)?[\d\.]*[df]` becomes `%s`,
/// keeping the `n$` argument index.
fn rewrite_numeric(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = String::with_capacity(v.len());
    let mut i = 0;
    let mut copied = 0;
    while i < b.len() {
        if b[i] != b'%' {
            i += 1;
            continue;
        }
        let Some((end, index)) = numeric_at(b, i) else {
            i += 1;
            continue;
        };
        out.push_str(&v[copied..i]);
        out.push('%');
        if let Some((s, e)) = index {
            out.push_str(&v[s..e]);
        }
        out.push('s');
        i = end;
        copied = end;
    }
    out.push_str(&v[copied..]);
    out
}

/// A `PATTERN` match starting at the `%` at `i`: its end, and the `\d+\$` group's range when
/// it took part. The group is tried first and given up if the rest fails, as the regex
/// backtracks.
fn numeric_at(b: &[u8], i: usize) -> Option<(usize, Option<(usize, usize)>)> {
    // `[\d\.]*[df]` from `j`; the star is greedy, and neither `d` nor `f` is in its class.
    let rest = |j: usize| {
        let k = j + b[j..].iter().take_while(|c| c.is_ascii_digit() || **c == b'.').count();
        matches!(b.get(k), Some(b'd' | b'f')).then_some(k + 1)
    };
    let d = i + 1 + b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
    if d > i + 1 && b.get(d) == Some(&b'$') {
        if let Some(end) = rest(d + 1) {
            return Some((end, Some((i + 1, d + 1))));
        }
    }
    rest(i + 1).map(|end| (end, None))
}

/// `String.format(fmt, args)` for the conversions lang values use; `None` where Java throws an
/// `IllegalFormatException` (a missing argument, `%d` given a string, a conversion not handled
/// here). Arguments past the last one used are ignored, as Java ignores them.
pub fn format(fmt: &str, args: &[Arg]) -> Option<String> {
    let b = fmt.as_bytes();
    let mut out = String::with_capacity(fmt.len());
    let mut ordinary = 0;
    let mut i = 0;
    let mut copied = 0;
    while i < b.len() {
        if b[i] != b'%' {
            i += 1;
            continue;
        }
        out.push_str(&fmt[copied..i]);
        let mut j = i + 1;
        let digits = |j: usize| j + b[j..].iter().take_while(|c| c.is_ascii_digit()).count();
        // `%n$`: an explicit argument index, counted from 1.
        let mut index = None;
        let d = digits(j);
        if d > j && b.get(d) == Some(&b'$') {
            index = Some(fmt[j..d].parse::<usize>().ok().filter(|&n| n >= 1)? - 1);
            j = d + 1;
        }
        let left = b.get(j) == Some(&b'-');
        if left {
            j += 1;
        }
        let d = digits(j);
        let width = if d > j { fmt[j..d].parse::<usize>().ok()? } else { 0 };
        j = d;
        let conv = *b.get(j)?;
        let text = match conv {
            b'%' | b'n' => {
                if index.is_some() {
                    return None;
                }
                if conv == b'%' { "%".to_string() } else { "\n".to_string() }
            }
            b's' | b'S' | b'd' => {
                let n = index.unwrap_or_else(|| {
                    ordinary += 1;
                    ordinary - 1
                });
                let arg = args.get(n)?;
                match (conv, arg) {
                    (b'd', Arg::Str(_)) => return None,
                    (b'S', a) => a.to_string().to_uppercase(),
                    (_, a) => a.to_string(),
                }
            }
            _ => return None,
        };
        // A `-` with no width is a MissingFormatWidthException.
        if left && width == 0 {
            return None;
        }
        let pad = width.saturating_sub(text.chars().count());
        if left {
            out.push_str(&text);
            out.extend(std::iter::repeat_n(' ', pad));
        } else {
            out.extend(std::iter::repeat_n(' ', pad));
            out.push_str(&text);
        }
        i = j + 1;
        copied = i;
    }
    out.push_str(&fmt[copied..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Minecraft en_us.lang, a few real lines\r\n\
        \r\n\
        gui.done=Done\r\n\
        death.attack.mob=%1$s was slain by %2$s\r\n\
        commands.time.added=Added %d to the time\n\
        stat.walkOneCm=%.2f km\n\
        chat.type.text=<%s> %s\n\
        translation.test.invalid=hi %\n\
        translation.test.complex=Prefix, %s%2$s again %s and %1$s lastly %s and also %1$s again!\n\
        key.with.equals=a=b\n\
        not a pair\n\
        gui.done=Done!\r\
        empty.value=\n";

    fn args(a: &[&str]) -> Vec<Arg> {
        a.iter().map(|&s| s.into()).collect()
    }

    #[test]
    fn parses_like_locale() {
        let lang = Lang::parse(SAMPLE);
        // A later line wins; a lone `\r` ends a line.
        assert_eq!(lang.translate("gui.done"), "Done!");
        assert_eq!(lang.translate("key.with.equals"), "a=b");
        assert_eq!(lang.translate("empty.value"), "");
        assert!(lang.has_key("empty.value"));
        assert!(!lang.has_key("not a pair"));
        assert!(!lang.has_key("# Minecraft en_us.lang, a few real lines"));
        assert_eq!(lang.translate("missing.key"), "missing.key");
        // PATTERN: numeric conversions become %s, keeping the argument index.
        assert_eq!(lang.translate("commands.time.added"), "Added %s to the time");
        assert_eq!(lang.translate("stat.walkOneCm"), "%s km");
        assert_eq!(lang.len(), 9);
    }

    #[test]
    fn rewrite_follows_the_regex() {
        assert_eq!(rewrite_numeric("%d"), "%s");
        assert_eq!(rewrite_numeric("%2$d of %1$.1f"), "%2$s of %1$s");
        assert_eq!(rewrite_numeric("%1d"), "%s");
        assert_eq!(rewrite_numeric("%%d"), "%%s");
        assert_eq!(rewrite_numeric("%12$x %s 100%"), "%12$x %s 100%");
        assert_eq!(rewrite_numeric("\u{e9}%.3f\u{e9}"), "\u{e9}%s\u{e9}");
    }

    #[test]
    fn formats_like_string_format() {
        let lang = Lang::parse(SAMPLE);
        assert_eq!(lang.format("death.attack.mob", &args(&["xy", "Zombie"])), "xy was slain by Zombie");
        assert_eq!(lang.format("chat.type.text", &args(&["xy", "hi"])), "<xy> hi");
        assert_eq!(lang.format("commands.time.added", &[1000.into()]), "Added 1000 to the time");
        // The vanilla translation test (TextComponentTranslation's): ordinary and positional
        // arguments count separately.
        assert_eq!(
            lang.format("translation.test.complex", &args(&["a", "b", "c", "d"])),
            "Prefix, ab again b and a lastly c and also a again!"
        );
        // Extra arguments are ignored, a key with no value formats to itself.
        assert_eq!(lang.format("gui.done", &args(&["x"])), "Done!");
        assert_eq!(lang.format("no.such.key", &[]), "no.such.key");
        // What String.format throws on.
        assert_eq!(lang.format("translation.test.invalid", &[]), "Format error: hi %");
        assert_eq!(lang.format("chat.type.text", &args(&["only one"])), "Format error: <%s> %s");
        assert_eq!(format("%d", &args(&["x"])), None);
        assert_eq!(format("%0$s", &args(&["x"])), None);
        assert_eq!(format("%-s", &args(&["x"])), None);
        assert_eq!(format("%q", &[]), None);
        // The rest of the handled conversions.
        assert_eq!(format("100%% %S%n", &args(&["ok"])).as_deref(), Some("100% OK\n"));
        assert_eq!(format("[%3s|%-4d]", &["a".into(), 7.into()]).as_deref(), Some("[  a|7   ]"));
        assert_eq!(format("%s", &[(-5i32).into()]).as_deref(), Some("-5"));
    }
}
