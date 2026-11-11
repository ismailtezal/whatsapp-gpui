//! Development tool: runs one read-only SQL query on a SQLite file and prints the rows.
//! Meant for counts and other aggregate questions; do not use it to print message text.
//! Usage: wa-sql <file.db> "<query>"

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path, query] = args.as_slice() else {
        anyhow::bail!("usage: wa-sql <file.db> \"<query>\"");
    };
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = db.prepare(query)?;
    let columns = statement.column_count();
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let cells: Vec<String> = (0..columns)
            .map(|index| match row.get::<_, Value>(index) {
                Ok(Value::Null) => "NULL".to_string(),
                Ok(Value::Integer(value)) => value.to_string(),
                Ok(Value::Real(value)) => value.to_string(),
                Ok(Value::Text(value)) => value,
                Ok(Value::Blob(value)) => format!("blob({})", value.len()),
                Err(error) => format!("error: {error}"),
            })
            .collect();
        println!("{}", cells.join(" | "));
    }
    Ok(())
}
