//! Uninstalled Lutris library discovery from `~/.local/share/lutris/pga.db` (Linux).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::{
    finish_library_game, LibraryGame, LibraryGameMeta, LibrarySection, LibrarySource,
};

#[cfg(target_os = "linux")]
pub fn scan_lutris_library() -> Option<LibrarySection> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));
    let cache_home = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".cache"));

    let db_path = data_home.join("lutris").join("pga.db");
    let cover_dir = cache_home.join("lutris").join("coverart");
    scan_lutris_db(&db_path, &cover_dir)
}

pub fn scan_lutris_db(db_path: &Path, cover_dir: &Path) -> Option<LibrarySection> {
    if !db_path.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;

    let total_owned: usize = conn
        .query_row("SELECT COUNT(*) FROM games WHERE slug IS NOT NULL AND name IS NOT NULL", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0)
        .max(0) as usize;

    let mut stmt = conn
        .prepare(
            "SELECT slug, name, service, platform FROM games \
             WHERE COALESCE(installed, 0) = 0 AND slug IS NOT NULL AND name IS NOT NULL",
        )
        .ok()?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .ok()?;

    let mut games: Vec<LibraryGame> = Vec::new();
    for row in rows.flatten() {
        let (slug, title, service, platform) = row;
        let slug = slug.trim().to_string();
        let title = title.trim().to_string();
        if slug.is_empty() || title.is_empty() {
            continue;
        }
        let cover_path = resolve_lutris_cover(cover_dir, &slug);
        let platforms = platform
            .map(|p| vec![p.to_ascii_lowercase()])
            .unwrap_or_else(|| vec!["linux".to_string()]);
        let metadata = LibraryGameMeta {
            lutris_slug: Some(slug.clone()),
            lutris_service: service.filter(|s| !s.trim().is_empty()),
            ..LibraryGameMeta::default()
        };
        games.push(finish_library_game(
            LibrarySource::Lutris,
            slug,
            title,
            cover_path,
            None,
            None,
            platforms,
            false,
            metadata,
        ));
    }

    if total_owned == 0 && games.is_empty() {
        return None;
    }

    Some(LibrarySection {
        source: LibrarySource::Lutris,
        label: LibrarySource::Lutris.label().to_string(),
        total_owned: total_owned.max(games.len()),
        games,
        warning: None,
    })
}

fn resolve_lutris_cover(cover_dir: &Path, slug: &str) -> Option<String> {
    for ext in ["jpg", "png", "webp"] {
        let p = cover_dir.join(format!("{slug}.{ext}"));
        if p.is_file() {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_uninstalled_games_from_lutris_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("pga.db");
        let cover_dir = dir.path().join("coverart");
        fs::create_dir_all(&cover_dir).unwrap();
        fs::write(cover_dir.join("witcher-3.jpg"), b"fake").unwrap();

        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE games (id INTEGER PRIMARY KEY, slug TEXT, name TEXT, service TEXT, platform TEXT, installed INTEGER)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO games (slug, name, service, platform, installed) VALUES ('witcher-3', 'The Witcher 3: Wild Hunt', 'gog', 'Windows', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO games (slug, name, service, platform, installed) VALUES ('hades', 'Hades', 'steam', 'Windows', 1)",
            [],
        )
        .unwrap();
        drop(conn);

        let section = scan_lutris_db(&db_path, &cover_dir).unwrap();
        assert_eq!(section.total_owned, 2);
        assert_eq!(section.games.len(), 1);
        assert_eq!(section.games[0].title, "The Witcher 3: Wild Hunt");
        assert_eq!(section.games[0].external_id, "witcher-3");
        assert!(section.games[0].cover_path.is_some());
    }
}
