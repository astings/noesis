//! Serialized metadata.json writes. Every writer goes through `METADATA_LOCK` so a
//! read-modify-write never loses another writer's keys, and each write uses its own
//! temp name so no writer renames another's half-written file.

use once_cell::sync::Lazy;
use serde_json::{Map, Value};
use std::io::{Error, ErrorKind, Result};
use std::path::Path;
use std::sync::Mutex;

/// Process-wide. Held only around synchronous file I/O, never across an .await.
pub static METADATA_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn invalid(msg: String) -> Error {
    Error::new(ErrorKind::InvalidData, msg)
}

fn write_atomic(folder: &Path, json: &str) -> Result<()> {
    let tmp = folder.join(format!(".metadata.json.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, folder.join("metadata.json")).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e
    })
}

/// Read metadata.json (`{}` only when the file is missing), let `f` change the root object,
/// then write it back. An unreadable file, invalid JSON, a non-object root or an Err from `f`
/// writes nothing.
pub fn update_metadata<T>(folder: &Path, f: impl FnOnce(&mut Map<String, Value>) -> Result<T>) -> Result<T> {
    let _guard = METADATA_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut root: Value = match std::fs::read_to_string(folder.join("metadata.json")) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| invalid(format!("metadata.json is not valid JSON: {}", e)))?,
        Err(e) if e.kind() == ErrorKind::NotFound => Value::Object(Map::new()),
        Err(e) => return Err(e),
    };
    let obj = root.as_object_mut().ok_or_else(|| invalid("metadata.json root is not an object".into()))?;
    let out = f(obj)?;
    write_atomic(folder, &serde_json::to_string_pretty(&root)?)?;
    Ok(out)
}

/// Replace metadata.json with `value`, under the same lock.
pub fn write_metadata<T: serde::Serialize>(folder: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    let _guard = METADATA_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    write_atomic(folder, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(dir: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join("metadata.json")).unwrap()).unwrap()
    }

    #[test]
    fn preserves_other_keys_and_creates_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        update_metadata(dir.path(), |o| { o.insert("a".into(), 1.into()); Ok(()) }).unwrap();
        update_metadata(dir.path(), |o| { o.insert("b".into(), 2.into()); Ok(()) }).unwrap();
        assert_eq!(read(dir.path()), serde_json::json!({"a": 1, "b": 2}));
    }

    #[test]
    fn unparseable_or_non_object_or_closure_err_leaves_file_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("metadata.json");
        for bad in ["{not json", "[1,2]"] {
            std::fs::write(&p, bad).unwrap();
            assert!(update_metadata(dir.path(), |o| { o.insert("x".into(), 1.into()); Ok(()) }).is_err());
            assert_eq!(std::fs::read_to_string(&p).unwrap(), bad);
        }
        std::fs::write(&p, r#"{"k":1}"#).unwrap();
        let r: Result<()> = update_metadata(dir.path(), |o| { o.insert("x".into(), 1.into()); Err(invalid("no".into())) });
        assert!(r.is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), r#"{"k":1}"#);
    }

    #[test]
    fn concurrent_updates_on_different_keys_all_land() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("metadata.json"), "{}").unwrap();
        let threads: Vec<_> = (0..16)
            .map(|i| {
                let d = dir.path().to_path_buf();
                std::thread::spawn(move || {
                    for j in 0..10 {
                        update_metadata(&d, |o| { o.insert(format!("k{}_{}", i, j), j.into()); Ok(()) }).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(read(dir.path()).as_object().unwrap().len(), 160);
        let leftovers = std::fs::read_dir(dir.path()).unwrap().filter(|e| e.as_ref().unwrap().file_name() != "metadata.json").count();
        assert_eq!(leftovers, 0);
    }
}
