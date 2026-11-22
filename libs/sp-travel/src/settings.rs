//! A strict settings file of `key=number` lines: every key known, written once, inside its bounds. Anything else
//! refuses the whole file, so a typing mistake never runs as a default the owner did not choose.

/// One setting: its key, the bounds its value must be inside, and the value of a file that does not name it.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub key: &'static str,
    pub min: u64,
    pub max: u64,
    pub default: u64,
}

/// The longest settings file read.
pub const MAX_BYTES: usize = 4096;

/// The values of `fields`, in their order. Empty lines and lines that start with `#` are skipped.
pub fn parse(text: &str, fields: &[Field]) -> Result<Vec<u64>, &'static str> {
    if text.len() > MAX_BYTES {
        return Err("config_error");
    }
    let mut values: Vec<u64> = fields.iter().map(|f| f.default).collect();
    let mut seen = vec![false; fields.len()];
    for line in text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
    {
        let (key, value) = line.split_once('=').ok_or("config_error")?;
        let at = fields
            .iter()
            .position(|f| f.key == key.trim())
            .ok_or("config_error")?;
        let value: u64 = value.trim().parse().map_err(|_| "config_error")?;
        let (Some(field), Some(slot), Some(mark)) =
            (fields.get(at), values.get_mut(at), seen.get_mut(at))
        else {
            return Err("config_error");
        };
        if *mark || value < field.min || value > field.max {
            return Err("config_error");
        }
        *mark = true;
        *slot = value;
    }
    Ok(values)
}

/// The file a first load writes: every field with its default.
pub fn default_text(fields: &[Field]) -> String {
    fields
        .iter()
        .map(|f| format!("{}={}\n", f.key, f.default))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELDS: [Field; 2] = [
        Field {
            key: "delay_seconds",
            min: 0,
            max: 60,
            default: 3,
        },
        Field {
            key: "max_homes",
            min: 1,
            max: 100,
            default: 5,
        },
    ];

    #[test]
    fn defaults_fill_what_a_file_does_not_name_and_the_default_file_reads_back() {
        assert_eq!(parse("", &FIELDS), Ok(vec![3, 5]));
        assert_eq!(
            parse("# note\n\n max_homes = 7 \n", &FIELDS),
            Ok(vec![3, 7])
        );
        assert_eq!(parse(&default_text(&FIELDS), &FIELDS), Ok(vec![3, 5]));
        assert_eq!(default_text(&FIELDS), "delay_seconds=3\nmax_homes=5\n");
    }

    #[test]
    fn unknown_duplicate_out_of_bounds_and_malformed_lines_refuse_the_file() {
        for bad in [
            "delay_seconds=61",
            "max_homes=0",
            "max_homes=101",
            "delay_seconds=-1",
            "delay_seconds=1.5",
            "delay_seconds",
            "x=3",
            "delay_seconds=1\ndelay_seconds=2",
            &format!("#{}", "x".repeat(MAX_BYTES)),
        ] {
            assert_eq!(parse(bad, &FIELDS), Err("config_error"), "{bad}");
        }
    }
}
