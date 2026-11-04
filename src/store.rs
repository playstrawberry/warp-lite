use crate::model::{Location, MAX_WARPS, Warps, name};
use std::{fs, io::Read, path::Path};

pub const MAX_FILE: u64 = 128 * 1024;

pub fn decode(text: &str) -> Result<Warps, &'static str> {
    if text.len() as u64 > MAX_FILE {
        return Err("data_error");
    }
    let mut lines = text.lines();
    if lines.next() != Some("warp-lite\t1") {
        return Err("data_error");
    }
    let mut warps = Warps::new();
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 7 || warps.len() >= MAX_WARPS {
            return Err("data_error");
        }
        let key = name(f[0]).filter(|n| n == f[0]).ok_or("data_error")?;
        let loc = Location {
            world: f[1].into(),
            pos: (
                f[2].parse().map_err(|_| "data_error")?,
                f[3].parse().map_err(|_| "data_error")?,
                f[4].parse().map_err(|_| "data_error")?,
            ),
            yaw: f[5].parse().map_err(|_| "data_error")?,
            pitch: f[6].parse().map_err(|_| "data_error")?,
        };
        if !loc.valid() || warps.insert(key, loc).is_some() {
            return Err("data_error");
        }
    }
    Ok(warps)
}

pub fn encode(warps: &Warps) -> Result<String, &'static str> {
    if warps.len() > MAX_WARPS {
        return Err("data_error");
    }
    let mut out = String::from("warp-lite\t1\n");
    for (key, loc) in warps {
        if name(key).as_ref() != Some(key) || !loc.valid() {
            return Err("data_error");
        }
        out.push_str(&format!(
            "{key}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            loc.world, loc.pos.0, loc.pos.1, loc.pos.2, loc.yaw, loc.pitch
        ));
    }
    if out.len() as u64 > MAX_FILE {
        return Err("data_error");
    }
    Ok(out)
}

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

pub fn load(dir: &Path) -> Result<Warps, &'static str> {
    read_optional(&dir.join("warps.tsv"), MAX_FILE)?
        .map_or_else(|| Ok(Warps::new()), |text| decode(&text))
}

pub fn save(dir: &Path, warps: &Warps) -> Result<(), &'static str> {
    let text = encode(warps)?;
    let tmp = dir.join("warps.tmp");
    let path = dir.join("warps.tsv");
    fs::write(&tmp, text)
        .and_then(|()| fs::rename(tmp, path))
        .map_err(|_| "save_error")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::loc;

    #[test]
    fn versioned_warps_roundtrip_exact_position_world_and_rotation() {
        let mut warps = Warps::new();
        warps.insert("spawn".into(), loc());
        assert_eq!(decode(&encode(&warps).unwrap()).unwrap(), warps);
    }

    #[test]
    fn corrupt_duplicate_oversized_and_unknown_version_data_are_refused() {
        for text in [
            "",
            "warp-lite\t2\n",
            "warp-lite\t1\nbad\n",
            "warp-lite\t1\nspawn\tworld\tNaN\t64\t0\t0\t0\n",
            "warp-lite\t1\nspawn\tworld\t1\t64\t0\t0\t0\nspawn\tworld\t1\t64\t0\t0\t0\n",
            &"x".repeat(MAX_FILE as usize + 1),
        ] {
            assert!(decode(text).is_err());
        }
    }

    #[test]
    fn atomic_save_restart_and_malformed_file_preservation() {
        let dir = std::env::temp_dir().join(format!("warp-lite-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut warps = Warps::new();
        warps.insert("spawn".into(), loc());
        save(&dir, &warps).unwrap();
        assert_eq!(load(&dir).unwrap(), warps);
        warps.insert("second".into(), loc());
        save(&dir, &warps).unwrap();
        assert_eq!(load(&dir).unwrap(), warps);
        std::fs::write(dir.join("warps.tsv"), "broken").unwrap();
        assert!(load(&dir).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("warps.tsv")).unwrap(),
            "broken"
        );
        std::fs::remove_file(dir.join("warps.tsv")).unwrap();
        assert!(load(&dir).unwrap().is_empty());
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn failed_atomic_write_never_replaces_the_previous_file() {
        let dir = std::env::temp_dir().join(format!("warp-lite-fail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut warps = Warps::new();
        warps.insert("spawn".into(), loc());
        save(&dir, &warps).unwrap();
        std::fs::create_dir(dir.join("warps.tmp")).unwrap();
        let before = std::fs::read(dir.join("warps.tsv")).unwrap();
        warps.insert("new".into(), loc());
        assert_eq!(save(&dir, &warps), Err("save_error"));
        assert_eq!(std::fs::read(dir.join("warps.tsv")).unwrap(), before);
        std::fs::remove_dir(dir.join("warps.tmp")).unwrap();
        std::fs::remove_file(dir.join("warps.tsv")).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
