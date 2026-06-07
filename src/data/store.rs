use rusqlite::{Connection, OptionalExtension, params};
use std::path::PathBuf;

pub struct Store {
    conn: Connection,
    sessions_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub struct ThreadMeta {
    pub id: i64,
    pub title: String,
    pub preview: String,
    pub session_file: Option<String>,
    pub model: Option<String>,
    pub pinned: bool,
    pub created_at: String,
    pub updated_at: String,
}

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "001_init",
        "
        CREATE TABLE threads (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            title         TEXT NOT NULL DEFAULT '',
            preview       TEXT NOT NULL DEFAULT '',
            session_file  TEXT,
            model         TEXT,
            pinned        INTEGER NOT NULL DEFAULT 0,
            created_at    DATETIME NOT NULL DEFAULT (datetime('now')),
            updated_at    DATETIME NOT NULL DEFAULT (datetime('now'))
        );
        ",
    ),
    (
        "002_workspaces",
        "
        CREATE TABLE workspaces (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL,
            path        TEXT NOT NULL UNIQUE,
            created_at  DATETIME NOT NULL DEFAULT (datetime('now')),
            updated_at  DATETIME NOT NULL DEFAULT (datetime('now'))
        );
        ",
    ),
];

impl Store {
    pub fn open() -> Result<Self, StoreError> {
        let dir = dirs::home_dir()
            .ok_or(StoreError::HomeDir)?
            .join(".mini-pi");
        std::fs::create_dir_all(&dir).map_err(StoreError::Io)?;
        std::fs::create_dir_all(dir.join("sessions")).map_err(StoreError::Io)?;

        let db_path = dir.join("mini-pi.db");
        let conn = Connection::open(&db_path).map_err(StoreError::Rusqlite)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .map_err(StoreError::Rusqlite)?;

        let store = Self {
            conn,
            sessions_dir: dir.join("sessions"),
        };
        store.run_migrations()?;
        Ok(store)
    }

    fn run_migrations(&self) -> Result<(), StoreError> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS _migrations (
                    name  TEXT PRIMARY KEY,
                    applied_at DATETIME NOT NULL DEFAULT (datetime('now'))
                );",
            )
            .map_err(StoreError::Rusqlite)?;

        for &(name, sql) in MIGRATIONS {
            let applied: bool = self
                .conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM _migrations WHERE name = ?1",
                    params![name],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap_or(false);

            if !applied {
                self.conn.execute_batch(sql).map_err(StoreError::Rusqlite)?;
                self.conn
                    .execute("INSERT INTO _migrations (name) VALUES (?1)", params![name])
                    .map_err(StoreError::Rusqlite)?;
            }
        }

        Ok(())
    }

    pub fn sessions_dir(&self) -> &PathBuf {
        &self.sessions_dir
    }

    pub fn create_thread(&self, title: &str, preview: &str) -> Result<ThreadMeta, StoreError> {
        self.conn
            .execute(
                "INSERT INTO threads (title, preview) VALUES (?1, ?2)",
                params![title, preview],
            )
            .map_err(StoreError::Rusqlite)?;
        let id = self.conn.last_insert_rowid();
        self.get_thread(id).map(|opt| opt.unwrap())
    }

    pub fn list_threads(&self) -> Result<Vec<ThreadMeta>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, title, preview, session_file, model, pinned, created_at, updated_at \
                 FROM threads ORDER BY pinned DESC, updated_at DESC",
            )
            .map_err(StoreError::Rusqlite)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ThreadMeta {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    preview: row.get(2)?,
                    session_file: row.get(3)?,
                    model: row.get(4)?,
                    pinned: row.get::<_, i32>(5)? != 0,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .map_err(StoreError::Rusqlite)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::Rusqlite)
    }

    pub fn get_thread(&self, id: i64) -> Result<Option<ThreadMeta>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, title, preview, session_file, model, pinned, created_at, updated_at \
                 FROM threads WHERE id = ?1",
            )
            .map_err(StoreError::Rusqlite)?;
        stmt.query_row(params![id], |row| {
            Ok(ThreadMeta {
                id: row.get(0)?,
                title: row.get(1)?,
                preview: row.get(2)?,
                session_file: row.get(3)?,
                model: row.get(4)?,
                pinned: row.get::<_, i32>(5)? != 0,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .optional()
        .map_err(StoreError::Rusqlite)
    }

    pub fn update_thread(
        &self,
        id: i64,
        title: Option<&str>,
        preview: Option<&str>,
        session_file: Option<Option<&str>>,
        model: Option<Option<&str>>,
        pinned: Option<bool>,
    ) -> Result<(), StoreError> {
        if let Some(t) = title {
            self.conn
                .execute(
                    "UPDATE threads SET title = ?1 WHERE id = ?2",
                    params![t, id],
                )
                .map_err(StoreError::Rusqlite)?;
        }
        if let Some(p) = preview {
            self.conn
                .execute(
                    "UPDATE threads SET preview = ?1 WHERE id = ?2",
                    params![p, id],
                )
                .map_err(StoreError::Rusqlite)?;
        }
        if let Some(sf) = session_file {
            self.conn
                .execute(
                    "UPDATE threads SET session_file = ?1 WHERE id = ?2",
                    params![sf, id],
                )
                .map_err(StoreError::Rusqlite)?;
        }
        if let Some(m) = model {
            self.conn
                .execute(
                    "UPDATE threads SET model = ?1 WHERE id = ?2",
                    params![m, id],
                )
                .map_err(StoreError::Rusqlite)?;
        }
        if let Some(p) = pinned {
            self.conn
                .execute(
                    "UPDATE threads SET pinned = ?1 WHERE id = ?2",
                    params![p as i32, id],
                )
                .map_err(StoreError::Rusqlite)?;
        }
        self.conn
            .execute(
                "UPDATE threads SET updated_at = datetime('now') WHERE id = ?1",
                params![id],
            )
            .map_err(StoreError::Rusqlite)?;
        Ok(())
    }

    pub fn create_workspace(&self, name: &str, path: &str) -> Result<WorkspaceMeta, StoreError> {
        self.conn
            .execute(
                "INSERT INTO workspaces (name, path) VALUES (?1, ?2)",
                params![name, path],
            )
            .map_err(StoreError::Rusqlite)?;
        let id = self.conn.last_insert_rowid();
        self.get_workspace(id).map(|opt| opt.unwrap())
    }

    pub fn list_workspaces(&self) -> Result<Vec<WorkspaceMeta>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, path, created_at, updated_at \
                 FROM workspaces ORDER BY updated_at DESC",
            )
            .map_err(StoreError::Rusqlite)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(WorkspaceMeta {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })
            .map_err(StoreError::Rusqlite)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::Rusqlite)
    }

    pub fn get_workspace(&self, id: i64) -> Result<Option<WorkspaceMeta>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, path, created_at, updated_at \
                 FROM workspaces WHERE id = ?1",
            )
            .map_err(StoreError::Rusqlite)?;
        stmt.query_row(params![id], |row| {
            Ok(WorkspaceMeta {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })
        .optional()
        .map_err(StoreError::Rusqlite)
    }

    pub fn delete_workspace(&self, id: i64) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM workspaces WHERE id = ?1", params![id])
            .map_err(StoreError::Rusqlite)?;
        Ok(())
    }

    pub fn default_workspace_dir(&self) -> PathBuf {
        self.sessions_dir
            .parent()
            .unwrap_or(&self.sessions_dir)
            .join("workspace")
    }

    pub fn toggle_pin(&self, id: i64) -> Result<bool, StoreError> {
        let current: bool = self
            .conn
            .query_row(
                "SELECT pinned FROM threads WHERE id = ?1",
                params![id],
                |row| row.get::<_, i32>(0),
            )
            .map(|v| v != 0)
            .map_err(StoreError::Rusqlite)?;
        let new_val = !current;
        self.conn
            .execute(
                "UPDATE threads SET pinned = ?1 WHERE id = ?2",
                params![new_val as i32, id],
            )
            .map_err(StoreError::Rusqlite)?;
        Ok(new_val)
    }

    pub fn delete_thread(&self, id: i64) -> Result<(), StoreError> {
        let session_file: Option<String> = self
            .conn
            .query_row(
                "SELECT session_file FROM threads WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(StoreError::Rusqlite)?
            .flatten();
        self.conn
            .execute("DELETE FROM threads WHERE id = ?1", params![id])
            .map_err(StoreError::Rusqlite)?;
        if let Some(sf) = session_file {
            let _ = std::fs::remove_file(self.sessions_dir.join(&sf));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceMeta {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug)]
pub enum StoreError {
    Rusqlite(rusqlite::Error),
    Io(std::io::Error),
    HomeDir,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Rusqlite(e) => write!(f, "database error: {}", e),
            StoreError::Io(e) => write!(f, "io error: {}", e),
            StoreError::HomeDir => write!(f, "could not determine home directory"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Rusqlite(e) => Some(e),
            StoreError::Io(e) => Some(e),
            StoreError::HomeDir => None,
        }
    }
}
