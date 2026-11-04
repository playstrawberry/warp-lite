use std::{collections::BTreeMap, sync::OnceLock};

const FILES: [(&str, &str); 4] = [
    ("en", include_str!("../lang/en.lang")),
    ("fr", include_str!("../lang/fr.lang")),
    ("es", include_str!("../lang/es.lang")),
    ("tr", include_str!("../lang/tr.lang")),
];
type Catalog = BTreeMap<&'static str, &'static str>;
static CATALOGS: OnceLock<[Catalog; 4]> = OnceLock::new();

pub fn text(locale: &str, key: &str, args: &[&str]) -> String {
    let catalogs = CATALOGS.get_or_init(|| {
        FILES.map(|(_, file)| {
            file.lines()
                .filter_map(|line| line.split_once('='))
                .collect()
        })
    });
    let code = locale.get(..2).unwrap_or("en");
    let index = FILES
        .iter()
        .position(|(c, _)| code.eq_ignore_ascii_case(c))
        .unwrap_or(0);
    let raw = catalogs[index]
        .get(key)
        .or_else(|| catalogs[0].get(key))
        .copied()
        .unwrap_or(key);
    // Single-pass substitution never interprets braces supplied by a player.
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        if let Some(end) = tail.find('}')
            && let Ok(index) = tail[1..end].parse::<usize>()
            && let Some(arg) = args.get(index)
        {
            out.push_str(arg);
            rest = &tail[end + 1..];
            continue;
        }
        out.push('{');
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(s: &str) -> std::collections::BTreeSet<&str> {
        s.split('{')
            .skip(1)
            .filter_map(|s| s.split_once('}').map(|(p, _)| p))
            .collect()
    }

    #[test]
    fn every_language_has_exact_keys_and_placeholders_without_duplicates() {
        let parsed = FILES.map(|(_, file)| {
            file.lines()
                .map(|line| line.split_once('=').unwrap())
                .collect::<BTreeMap<_, _>>()
        });
        for (i, catalog) in parsed.iter().enumerate() {
            assert_eq!(catalog.len(), FILES[i].1.lines().count());
            assert_eq!(
                catalog.keys().collect::<Vec<_>>(),
                parsed[0].keys().collect::<Vec<_>>()
            );
            for (key, value) in catalog {
                assert!(!value.is_empty());
                assert_eq!(placeholders(value), placeholders(parsed[0][key]));
            }
        }
    }

    #[test]
    fn locales_fallback_and_user_arguments_are_not_templates() {
        assert_eq!(
            text("unknown", "saved", &["spawn"]),
            text("en", "saved", &["spawn"])
        );
        assert_eq!(
            text("Fr_fr", "saved", &["spawn"]),
            text("fr", "saved", &["spawn"])
        );
        assert!(text("en", "waiting", &["{1}", "3"]).contains("{1}"));
        assert_ne!(
            text("tr", "saved", &["spawn"]),
            text("en", "saved", &["spawn"])
        );
    }
}
