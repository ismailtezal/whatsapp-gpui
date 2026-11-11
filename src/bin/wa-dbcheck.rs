//! Development check: is a SQLite file intact? Opens it read-only and runs SQLite's own
//! integrity check on each table separately (so one broken table does not hide the
//! state of the others), then prints how many rows each table holds.
//! Usage: wa-dbcheck <file.db>

use rusqlite::{Connection, OpenFlags};

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).ok_or_else(|| anyhow::anyhow!("usage: wa-dbcheck <file.db>"))?;
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    for table in &tables {
        let check = db
            .prepare(&format!("PRAGMA integrity_check(\"{table}\")"))
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, _>>()
            });
        let count: Result<i64, _> = db.query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| row.get(0));
        let check = match check {
            Ok(lines) => lines.into_iter().take(3).collect::<Vec<_>>().join("; "),
            Err(error) => format!("CHECK FAILED: {error}"),
        };
        let count = match count {
            Ok(count) => count.to_string(),
            Err(error) => format!("COUNT FAILED: {error}"),
        };
        println!("{table}: rows={count} check={check}");
    }
    let freelist: i64 = db.query_row("PRAGMA freelist_count", [], |row| row.get(0)).unwrap_or(-1);
    println!("free pages: {freelist}");
    Ok(())
}
