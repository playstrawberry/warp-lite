//! Small files of a script's data folder: bounded reads, writes that replace the file whole, and the one stored
//! form of a list of named locations.
use crate::{Location, name};
use std::{collections::BTreeMap, fs, io::Read, path::Path};

/// Named locations, sorted by name.
pub type Locations = BTreeMap<String, Location>;

/// What one list file may hold. `header` is its first line (the script's name and the format's version).
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub header: &'static str,
    pub max_entries: usize,
    pub max_bytes: u64,
}

/// Reads a file of at most `limit` bytes of UTF-8; `None` when it does not exist. A longer file, bytes that are
/// not UTF-8 or any other failure is `data_error`, never a partial text.
pub fn read_optional(path: &Path, limit: u64) -> Result<Option<String>, &'static str> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("data_error"),
    };
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "data_error")?;
    if bytes.len() as u64 > limit {
        return Err("data_error");
    }
    String::from_utf8(bytes).map(Some).map_err(|_| "data_error")
}

/// Writes `text` to `tmp` and renames it over `path`, so a reader sees the old file or the new one, never a half.
/// A rename is not a promise against power loss.
pub fn replace(tmp: &Path, path: &Path, text: &str) -> Result<(), &'static str> {
    fs::write(tmp, text)
        .and_then(|()| fs::rename(tmp, path))
        .map_err(|_| "save_error")
}

/// The list as text: the header, then one line per entry: name, world, x, y, z, yaw, pitch, tab-separated.
pub fn encode(shape: Shape, entries: &Locations) -> Result<String, &'static str> {
    if entries.len() > shape.max_entries {
        return Err("data_error");
    }
    let mut out = format!("{}\n", shape.header);
    for (key, loc) in entries {
        if name(key).as_ref() != Some(key) || !loc.valid() {
            return Err("data_error");
        }
        out.push_str(&format!(
            "{key}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            loc.world, loc.pos.0, loc.pos.1, loc.pos.2, loc.yaw, loc.pitch
        ));
    }
    if out.len() as u64 > shape.max_bytes {
        return Err("data_error");
    }
    Ok(out)
}

/// Reads a list back. Another header, a malformed line, a name that is not in its stored form, a name twice, a
/// location that is not valid or more entries than the shape allows refuse the whole file.
pub fn decode(shape: Shape, text: &str) -> Result<Locations, &'static str> {
    if text.len() as u64 > shape.max_bytes {
        return Err("data_error");
    }
    let mut lines = text.lines();
    if lines.next() != Some(shape.header) {
        return Err("data_error");
    }
    let mut entries = Locations::new();
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        let [key, world, x, y, z, yaw, pitch] = f.as_slice() else {
            return Err("data_error");
        };
        if entries.len() >= shape.max_entries {
            return Err("data_error");
        }
        let key = name(key).filter(|n| n == key).ok_or("data_error")?;
        let loc = Location {
            world: (*world).into(),
            pos: (
                x.parse().map_err(|_| "data_error")?,
                y.parse().map_err(|_| "data_error")?,
                z.parse().map_err(|_| "data_error")?,
            ),
            yaw: yaw.parse().map_err(|_| "data_error")?,
            pitch: pitch.parse().map_err(|_| "data_error")?,
        };
        if !loc.valid() || entries.insert(key, loc).is_some() {
            return Err("data_error");
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::tests::loc;

    const SHAPE: Shape = Shape {
        header: "demo\t1",
        max_entries: 2,
        max_bytes: 1024,
    };

    #[test]
    fn a_list_roundtrips_exact_position_world_and_rotation() {
        let mut entries = Locations::new();
        entries.insert("spawn".into(), loc());
        let mut far = loc();
        far.pos = (-29_999_999.5, -64.0, 0.000_000_1);
        far.world = "minecraft:the_nether".into();
        entries.insert("far".into(), far);
        assert_eq!(
            decode(SHAPE, &encode(SHAPE, &entries).unwrap()).unwrap(),
            entries
        );
    }

    #[test]
    fn corrupt_duplicate_oversized_and_unknown_version_data_are_refused() {
        for text in [
            "",
            "demo\t2\n",
            "other\t1\n",
            "demo\t1\nbad\n",
            "demo\t1\nspawn\tworld\tNaN\t64\t0\t0\t0\n",
            "demo\t1\nSpawn\tworld\t1\t64\t0\t0\t0\n",
            "demo\t1\nspawn\tworld\t1\t64\t0\t0\t0\t9\n",
            "demo\t1\nspawn\tworld\t1\t64\t0\t0\t0\nspawn\tworld\t1\t64\t0\t0\t0\n",
            "demo\t1\na\tworld\t1\t64\t0\t0\t0\nb\tworld\t1\t64\t0\t0\t0\nc\tworld\t1\t64\t0\t0\t0\n",
            &"x".repeat(1025),
        ] {
            assert!(decode(SHAPE, text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn a_list_over_its_shape_or_with_a_bad_entry_is_never_written() {
        let mut entries = Locations::new();
        for key in ["a", "b", "c"] {
            entries.insert(key.into(), loc());
        }
        assert_eq!(encode(SHAPE, &entries), Err("data_error"));
        let mut entries = Locations::new();
        entries.insert("Bad Name".into(), loc());
        assert_eq!(encode(SHAPE, &entries), Err("data_error"));
        let mut entries = Locations::new();
        let mut bad = loc();
        bad.world = "a\tb".into();
        entries.insert("a".into(), bad);
        assert_eq!(encode(SHAPE, &entries), Err("data_error"));
    }

    #[test]
    fn a_read_is_bounded_and_a_replace_leaves_no_half_file() {
        let dir = std::env::temp_dir().join(format!("sp-travel-files-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("list.tsv");
        assert_eq!(read_optional(&path, 16), Ok(None));
        replace(&dir.join("list.tmp"), &path, "0123456789abcdef").unwrap();
        assert_eq!(
            read_optional(&path, 16).unwrap().as_deref(),
            Some("0123456789abcdef")
        );
        assert_eq!(read_optional(&path, 15), Err("data_error"));
        assert!(!dir.join("list.tmp").exists());
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert_eq!(read_optional(&path, 16), Err("data_error"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
