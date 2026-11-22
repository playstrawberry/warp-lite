//! The warp list's file: `warps.tsv`, in `sp-travel`'s list form.
use crate::model::{MAX_WARPS, Warps};
pub use sp_travel::files::read_optional;
use sp_travel::files::{self, Shape};
use std::path::Path;

pub const MAX_FILE: u64 = 128 * 1024;
const SHAPE: Shape = Shape {
    header: "warp-lite\t1",
    max_entries: MAX_WARPS,
    max_bytes: MAX_FILE,
};

pub fn decode(text: &str) -> Result<Warps, &'static str> {
    files::decode(SHAPE, text)
}

pub fn encode(warps: &Warps) -> Result<String, &'static str> {
    files::encode(SHAPE, warps)
}

pub fn load(dir: &Path) -> Result<Warps, &'static str> {
    read_optional(&dir.join("warps.tsv"), MAX_FILE)?
        .map_or_else(|| Ok(Warps::new()), |text| decode(&text))
}

pub fn save(dir: &Path, warps: &Warps) -> Result<(), &'static str> {
    files::replace(
        &dir.join("warps.tmp"),
        &dir.join("warps.tsv"),
        &encode(warps)?,
    )
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
