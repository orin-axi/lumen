use camino::Utf8PathBuf;
use lumen_store::SqliteStore;
use tempfile::{tempdir, TempDir};

pub struct TestDatabaseDouble {
    pub dir: TempDir,
    pub db_path: Utf8PathBuf,
    pub store: SqliteStore,
}

pub fn create_migrated_test_db() -> TestDatabaseDouble {
    let dir = tempdir().expect("Failed to create temporary directory for test DB");
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("lumen_test_fixture.db"))
        .expect("Failed to convert temp path to Utf8PathBuf");

    let store = SqliteStore::open(&db_path).expect("Failed to initialize migrated test SQLite store");

    TestDatabaseDouble { dir, db_path, store }
}
