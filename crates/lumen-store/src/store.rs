use camino::Utf8Path;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OpenFlags;
use std::time::Duration;

use crate::error::StoreError;
use crate::migrations::MigrationManager;

#[derive(Clone)]
pub struct SqliteStore {
    pool: Pool<SqliteConnectionManager>,
    is_read_only: bool,
}

impl SqliteStore {
    /// Opens or creates SQLite store in WAL concurrency mode.
    pub fn open(path: &Utf8Path) -> Result<Self, StoreError> {
        let manager = SqliteConnectionManager::file(path.as_std_path())
            .with_flags(OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI)
            .with_init(|c| {
                c.execute_batch(
                    "PRAGMA journal_mode = WAL;
                     PRAGMA foreign_keys = ON;
                     PRAGMA busy_timeout = 5000;
                     PRAGMA synchronous = NORMAL;",
                )
            });

        let pool = Pool::builder()
            .max_size(16)
            .connection_timeout(Duration::from_millis(5000))
            .build(manager)
            .map_err(StoreError::Pool)?;

        let store = Self { pool, is_read_only: false };

        store.run_migrations()?;
        Ok(store)
    }

    /// Opens existing SQLite store in read-only mode with query_only = ON.
    pub fn open_read_only(path: &Utf8Path) -> Result<Self, StoreError> {
        let manager = SqliteConnectionManager::file(path.as_std_path())
            .with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI)
            .with_init(|c| {
                c.execute_batch(
                    "PRAGMA foreign_keys = ON;
                     PRAGMA query_only = ON;
                     PRAGMA busy_timeout = 5000;",
                )
            });

        let pool = Pool::builder()
            .max_size(8)
            .connection_timeout(Duration::from_millis(5000))
            .build(manager)
            .map_err(StoreError::Pool)?;

        Ok(Self { pool, is_read_only: true })
    }

    /// Runs all pending schema migrations from V1 to V5.
    pub fn run_migrations(&self) -> Result<usize, StoreError> {
        if self.is_read_only {
            return Err(StoreError::ReadOnlyViolation);
        }

        let mut conn = self.pool.get().map_err(StoreError::Pool)?;
        MigrationManager::apply_migrations(&mut conn)
    }

    /// Obtains a pooled SQLite connection.
    pub fn connection(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, StoreError> {
        self.pool.get().map_err(StoreError::Pool)
    }

    pub fn is_read_only(&self) -> bool {
        self.is_read_only
    }
}
