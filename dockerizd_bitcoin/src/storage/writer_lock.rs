//! Kernel lock for all supported mutating commands on the dedicated Bitcoin VM.
//! Never unlink a lock file: every process must open the same inode.
use anyhow::{Context, Result};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};

pub struct WriterLock {
    _file: File,
}
impl WriterLock {
    pub fn for_database(database: &str) -> Result<Self> {
        let directory = std::env::var_os("BITCOIN_WRITER_LOCK_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("aml-bitcoin-writer"));
        Self::acquire(&directory, database)
    }
    pub fn acquire(directory: &Path, database: &str) -> Result<Self> {
        aml_warehouse::contract::identifier(database)?;
        std::fs::create_dir_all(directory)
            .context("cannot create Bitcoin writer lock directory")?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(format!("{database}.lock")))
            .context("cannot open Bitcoin writer lock")?;
        file.try_lock().map_err(|_| anyhow::anyhow!("Bitcoin writer ownership unavailable; another ingest/once/backfill/schema command may own this database, or the shared lock filesystem is unavailable"))?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::{Child, Command},
        time::{Duration, Instant},
    };

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn process_child() {
        let Some(directory) = std::env::var_os("AML_TEST_WRITER_LOCK_DIRECTORY") else {
            return;
        };
        let path = std::path::PathBuf::from(directory);
        let _owner = WriterLock::acquire(&path, "process_fixture").unwrap();
        std::fs::write(path.join("ready"), b"ready").unwrap();
        std::thread::sleep(Duration::from_secs(120));
    }

    #[test]
    fn competing_process_is_rejected_and_crash_releases_ownership() {
        let path =
            std::env::temp_dir().join(format!("bitcoin-lock-process-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let mut child = ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "storage::writer_lock::tests::process_child",
                    "--nocapture",
                ])
                .env("AML_TEST_WRITER_LOCK_DIRECTORY", &path)
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.join("ready").exists() {
            assert!(
                Instant::now() < deadline,
                "child did not acquire writer lock"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(WriterLock::acquire(&path, "process_fixture").is_err());
        let independent = WriterLock::acquire(&path, "other_database").unwrap();
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let recovered = WriterLock::acquire(&path, "process_fixture").unwrap();
        assert!(WriterLock::acquire(&path, "process_fixture").is_err());
        drop(recovered);
        drop(independent);
        std::fs::remove_dir_all(path).unwrap();
    }
}
