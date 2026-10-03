use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};

pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    ensure!(
        fs::symlink_metadata(path)?.is_dir(),
        "wallet directory must not be a symlink"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn private_file(path: &Path) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        ensure!(
            meta.is_file(),
            "wallet state must be a regular file: {}",
            path.display()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
}

pub fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    sync_parent(path)?;
    Ok(())
}

pub fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path.parent().context("missing parent directory")?)?.sync_all()?;
    Ok(())
}

pub fn lock(dir: &Path) -> Result<File> {
    let path = dir.join("cli.lock");
    private_file(&path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    file.try_lock_exclusive()
        .context("another bark-degen command is using this wallet")?;
    Ok(file)
}

pub struct Store(Connection);

impl Store {
    pub fn open(dir: &Path) -> Result<Self> {
        let path = dir.join("bets.sqlite");
        if !path.exists() {
            write_new(&path, &[])?;
        }
        private_file(&path)?;
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE;")?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(version <= 1, "journal was created by a newer CLI");
        db.execute_batch(
            "BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS operations (
                id TEXT PRIMARY KEY, kind TEXT NOT NULL, state TEXT NOT NULL,
                payload TEXT NOT NULL, created_at INTEGER NOT NULL DEFAULT(unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT(unixepoch())
            ); PRAGMA user_version=1; COMMIT;",
        )?;
        Ok(Self(db))
    }

    pub fn insert<T: Serialize>(&self, id: &str, kind: &str, state: &str, value: &T) -> Result<()> {
        self.0
            .execute(
                "INSERT INTO operations(id,kind,state,payload) VALUES(?1,?2,?3,?4)",
                params![id, kind, state, serde_json::to_string(value)?],
            )
            .context("operation ID already exists or journal could not be written")?;
        Ok(())
    }

    pub fn save<T: Serialize>(&self, id: &str, state: &str, value: &T) -> Result<()> {
        ensure!(
            self.0.execute(
                "UPDATE operations SET state=?2,payload=?3,updated_at=unixepoch() WHERE id=?1",
                params![id, state, serde_json::to_string(value)?]
            )? == 1,
            "operation not found"
        );
        Ok(())
    }

    pub fn get<T: DeserializeOwned>(&self, id: &str, kind: &str) -> Result<Option<(String, T)>> {
        let row: Option<(String, String)> = self
            .0
            .query_row(
                "SELECT state,payload FROM operations WHERE id=?1 AND kind=?2",
                params![id, kind],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(state, data)| Ok((state, serde_json::from_str(&data)?)))
            .transpose()
    }

    pub fn list(&self) -> Result<Vec<(String, String, String)>> {
        self.0
            .prepare("SELECT id,kind,state FROM operations ORDER BY created_at DESC, rowid DESC")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payment_intent_survives_restart_and_cannot_be_inserted_twice() {
        let temp = tempfile::tempdir().unwrap();
        {
            let store = Store::open(temp.path()).unwrap();
            store
                .insert("one", "withdraw", "sending", &"destination")
                .unwrap();
            assert!(
                store
                    .insert("one", "withdraw", "sending", &"other")
                    .is_err()
            );
        }
        let store = Store::open(temp.path()).unwrap();
        assert_eq!(
            store.get::<String>("one", "withdraw").unwrap().unwrap(),
            ("sending".into(), "destination".into())
        );
        assert!(store.get::<String>("one", "play").unwrap().is_none());
    }

    #[test]
    fn concurrent_wallet_commands_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let first = lock(temp.path()).unwrap();
        assert!(lock(temp.path()).is_err());
        drop(first);
        assert!(lock(temp.path()).is_ok());
    }
}
