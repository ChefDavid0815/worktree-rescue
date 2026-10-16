//! Content-addressable blob storage.
//!
//! Layout: `<root>/sha256/<aa>/<bb>/<full-hex>`. A blob file's name *is* the
//! SHA-256 of its bytes, so integrity can always be re-checked without the
//! database. New blobs are written to `<root>/tmp`, flushed to disk, hashed
//! while streaming and only then renamed into place.

use crate::error::{CoreError, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

const CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
}

/// Result of storing a blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBlob {
    pub sha256: String,
    pub size: u64,
    /// True if the content was not already present (counts toward new bytes).
    pub newly_written: bool,
}

impl BlobStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("sha256")).map_err(|e| CoreError::io(&root, e))?;
        fs::create_dir_all(root.join("tmp")).map_err(|e| CoreError::io(&root, e))?;
        Ok(BlobStore { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_for(&self, sha: &str) -> Result<PathBuf> {
        if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CoreError::Other(format!("invalid blob id {sha}")));
        }
        let sha = sha.to_ascii_lowercase();
        Ok(self
            .root
            .join("sha256")
            .join(&sha[0..2])
            .join(&sha[2..4])
            .join(&sha))
    }

    pub fn contains(&self, sha: &str) -> bool {
        self.path_for(sha).map(|p| p.is_file()).unwrap_or(false)
    }

    /// Store bytes already in memory.
    pub fn put_bytes(&self, data: &[u8]) -> Result<StoredBlob> {
        let sha = hex::encode(Sha256::digest(data));
        let size = data.len() as u64;
        let dest = self.path_for(&sha)?;
        if dest.is_file() {
            return Ok(StoredBlob {
                sha256: sha,
                size,
                newly_written: false,
            });
        }
        let tmp = self.tmp_path();
        {
            let mut f = File::create(&tmp).map_err(|e| CoreError::io(&tmp, e))?;
            f.write_all(data).map_err(|e| CoreError::io(&tmp, e))?;
            f.sync_all().map_err(|e| CoreError::io(&tmp, e))?;
        }
        self.commit_tmp(&tmp, &sha, size)
    }

    /// Stream a file from disk into the store. Returns the hash of the bytes
    /// that were actually read; callers compare file metadata before and
    /// after to detect concurrent modification.
    pub fn put_file(&self, src: &Path) -> Result<StoredBlob> {
        let file = File::open(src).map_err(|e| CoreError::io(src, e))?;
        let mut reader = BufReader::with_capacity(CHUNK, file);
        let tmp = self.tmp_path();
        let mut out = File::create(&tmp).map_err(|e| CoreError::io(&tmp, e))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; CHUNK];
        let mut size = 0u64;
        let res: Result<()> = (|| {
            loop {
                let n = reader.read(&mut buf).map_err(|e| CoreError::io(src, e))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
                out.write_all(&buf[..n])
                    .map_err(|e| CoreError::io(&tmp, e))?;
                size += n as u64;
            }
            out.sync_all().map_err(|e| CoreError::io(&tmp, e))?;
            Ok(())
        })();
        drop(out);
        if let Err(e) = res {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        let sha = hex::encode(hasher.finalize());
        if self.contains(&sha) {
            let _ = fs::remove_file(&tmp);
            return Ok(StoredBlob {
                sha256: sha,
                size,
                newly_written: false,
            });
        }
        self.commit_tmp(&tmp, &sha, size)
    }

    fn commit_tmp(&self, tmp: &Path, sha: &str, size: u64) -> Result<StoredBlob> {
        let dest = self.path_for(sha)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
        }
        match fs::rename(tmp, &dest) {
            Ok(()) => {}
            Err(e) => {
                let _ = fs::remove_file(tmp);
                // A concurrent writer may have produced the identical blob.
                if !dest.is_file() {
                    return Err(CoreError::io(&dest, e));
                }
                return Ok(StoredBlob {
                    sha256: sha.to_string(),
                    size,
                    newly_written: false,
                });
            }
        }
        Ok(StoredBlob {
            sha256: sha.to_string(),
            size,
            newly_written: true,
        })
    }

    fn tmp_path(&self) -> PathBuf {
        self.root
            .join("tmp")
            .join(format!("{}.part", uuid::Uuid::new_v4().simple()))
    }

    /// Re-hash a stored blob and confirm it matches its address.
    pub fn verify(&self, sha: &str) -> Result<u64> {
        let p = self.path_for(sha)?;
        let (actual, size) = hash_file(&p)?;
        if !actual.eq_ignore_ascii_case(sha) {
            return Err(CoreError::Integrity {
                what: format!("blob {sha}"),
                expected: sha.to_string(),
                actual,
            });
        }
        Ok(size)
    }

    pub fn read(&self, sha: &str) -> Result<Vec<u8>> {
        let p = self.path_for(sha)?;
        fs::read(&p).map_err(|e| CoreError::io(&p, e))
    }

    /// Read at most `limit` bytes (for previews).
    pub fn read_prefix(&self, sha: &str, limit: usize) -> Result<(Vec<u8>, bool)> {
        let p = self.path_for(sha)?;
        let f = File::open(&p).map_err(|e| CoreError::io(&p, e))?;
        let mut buf = Vec::new();
        let n = f
            .take(limit as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| CoreError::io(&p, e))?;
        let truncated = n > limit;
        buf.truncate(limit);
        Ok((buf, truncated))
    }

    /// Copy a blob to `dest` (which must not exist), verifying the hash of
    /// the bytes written. Uses a temp file in the destination directory and a
    /// rename so a crash never leaves a half-written file under the final name.
    pub fn export_to(&self, sha: &str, dest: &Path) -> Result<u64> {
        let src = self.path_for(sha)?;
        let parent = dest
            .parent()
            .ok_or_else(|| CoreError::InvalidTarget(dest.display().to_string()))?;
        fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
        let tmp = parent.join(format!(".wr-{}.part", uuid::Uuid::new_v4().simple()));
        let res: Result<u64> = (|| {
            let mut input = File::open(&src).map_err(|e| CoreError::io(&src, e))?;
            let mut out = File::create_new(&tmp).map_err(|e| CoreError::io(&tmp, e))?;
            let mut hasher = Sha256::new();
            let mut buf = vec![0u8; CHUNK];
            let mut size = 0u64;
            loop {
                let n = input.read(&mut buf).map_err(|e| CoreError::io(&src, e))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
                out.write_all(&buf[..n])
                    .map_err(|e| CoreError::io(&tmp, e))?;
                size += n as u64;
            }
            out.sync_all().map_err(|e| CoreError::io(&tmp, e))?;
            let actual = hex::encode(hasher.finalize());
            if !actual.eq_ignore_ascii_case(sha) {
                return Err(CoreError::Integrity {
                    what: format!("blob {sha} while exporting"),
                    expected: sha.to_string(),
                    actual,
                });
            }
            Ok(size)
        })();
        match res {
            Ok(size) => {
                if dest.exists() {
                    let _ = fs::remove_file(&tmp);
                    return Err(CoreError::InvalidTarget(format!(
                        "{} already exists",
                        dest.display()
                    )));
                }
                fs::rename(&tmp, dest).map_err(|e| {
                    let _ = fs::remove_file(&tmp);
                    CoreError::io(dest, e)
                })?;
                Ok(size)
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(e)
            }
        }
    }

    pub fn delete(&self, sha: &str) -> Result<()> {
        let p = self.path_for(sha)?;
        match fs::remove_file(&p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(CoreError::io(&p, e)),
        }
    }

    /// Remove leftover temp files from interrupted writes.
    pub fn clean_tmp(&self) -> usize {
        let mut n = 0;
        if let Ok(rd) = fs::read_dir(self.root.join("tmp")) {
            for e in rd.flatten() {
                if fs::remove_file(e.path()).is_ok() {
                    n += 1;
                }
            }
        }
        n
    }

    /// Every blob id present on disk.
    pub fn list_all(&self) -> Vec<String> {
        let mut out = Vec::new();
        let base = self.root.join("sha256");
        let Ok(l1) = fs::read_dir(&base) else {
            return out;
        };
        for a in l1.flatten() {
            let Ok(l2) = fs::read_dir(a.path()) else {
                continue;
            };
            for b in l2.flatten() {
                let Ok(l3) = fs::read_dir(b.path()) else {
                    continue;
                };
                for f in l3.flatten() {
                    if let Some(name) = f.file_name().to_str()
                        && name.len() == 64
                    {
                        out.push(name.to_string());
                    }
                }
            }
        }
        out
    }
}

/// SHA-256 of a file on disk, streaming.
pub fn hash_file(p: &Path) -> Result<(String, u64)> {
    let f = File::open(p).map_err(|e| CoreError::io(p, e))?;
    let mut reader = BufReader::with_capacity(CHUNK, f);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut size = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| CoreError::io(p, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((hex::encode(hasher.finalize()), size))
}

pub fn hash_bytes(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_dedupes_and_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let a = store.put_bytes(b"hello").unwrap();
        assert!(a.newly_written);
        let b = store.put_bytes(b"hello").unwrap();
        assert!(!b.newly_written);
        assert_eq!(a.sha256, b.sha256);
        assert_eq!(store.verify(&a.sha256).unwrap(), 5);
        // Corrupt and detect.
        fs::write(store.path_for(&a.sha256).unwrap(), b"HELLO").unwrap();
        assert!(matches!(
            store.verify(&a.sha256),
            Err(CoreError::Integrity { .. })
        ));
    }

    #[test]
    fn put_file_and_export() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path().join("v")).unwrap();
        let src = dir.path().join("src.bin");
        fs::write(&src, vec![7u8; 600_000]).unwrap();
        let b = store.put_file(&src).unwrap();
        assert_eq!(b.size, 600_000);
        let dest = dir.path().join("out").join("x.bin");
        store.export_to(&b.sha256, &dest).unwrap();
        assert_eq!(hash_file(&dest).unwrap().0, b.sha256);
        // Refuses to overwrite.
        assert!(store.export_to(&b.sha256, &dest).is_err());
    }

    #[test]
    fn rejects_bad_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        assert!(store.path_for("../../etc").is_err());
        assert!(store.path_for("zz").is_err());
    }
}
