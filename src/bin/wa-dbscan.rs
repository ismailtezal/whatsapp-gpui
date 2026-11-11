//! Development tool: reads a SQLite file page by page, without SQLite, to see what is
//! in it when SQLite itself refuses to open it.
//!
//! `wa-dbscan <file.db>` walks the schema table from page 1 and prints each entry it can
//! read (type, name, root page) and where the walk breaks; then counts the pages of
//! each kind in the whole file.

use std::collections::BTreeMap;

struct Db {
    bytes: Vec<u8>,
    page_size: usize,
}

fn varint(data: &[u8], mut at: usize) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    for index in 0..9 {
        let byte = *data.get(at)?;
        at += 1;
        if index == 8 {
            value = (value << 8) | byte as u64;
            return Some((value, at));
        }
        value = (value << 7) | (byte & 0x7f) as u64;
        if byte & 0x80 == 0 {
            return Some((value, at));
        }
    }
    None
}

impl Db {
    fn pages(&self) -> usize {
        self.bytes.len() / self.page_size
    }

    fn page(&self, number: usize) -> Option<&[u8]> {
        if number == 0 || number > self.pages() {
            return None;
        }
        let start = (number - 1) * self.page_size;
        Some(&self.bytes[start..start + self.page_size])
    }

    /// The payload of a table leaf cell, following overflow pages.
    fn payload(&self, page: &[u8], cell: usize) -> Option<(u64, Vec<u8>)> {
        let (size, at) = varint(page, cell)?;
        let (rowid, at) = varint(page, at)?;
        let size = size as usize;
        let usable = self.page_size;
        let max_local = usable - 35;
        let min_local = (usable - 12) * 32 / 255 - 23;
        let local = if size <= max_local {
            size
        } else {
            let k = min_local + (size - min_local) % (usable - 4);
            if k <= max_local { k } else { min_local }
        };
        let mut data = page.get(at..at + local)?.to_vec();
        if local < size {
            let mut next = u32::from_be_bytes(page.get(at + local..at + local + 4)?.try_into().ok()?) as usize;
            while data.len() < size && next != 0 {
                let overflow = self.page(next)?;
                next = u32::from_be_bytes(overflow[..4].try_into().ok()?) as usize;
                let take = (size - data.len()).min(usable - 4);
                data.extend_from_slice(&overflow[4..4 + take]);
            }
        }
        Some((rowid, data))
    }

    /// Text and integer columns of a record, as strings.
    fn record(&self, data: &[u8]) -> Option<Vec<String>> {
        let (header_len, mut at) = varint(data, 0)?;
        let mut body = header_len as usize;
        let mut out = Vec::new();
        while at < header_len as usize {
            let (kind, next) = varint(data, at)?;
            at = next;
            let (len, text) = match kind {
                0 => (0, "NULL".to_string()),
                1..=6 => {
                    let len = [0, 1, 2, 3, 4, 6, 8][kind as usize];
                    let mut value: i64 = 0;
                    for byte in data.get(body..body + len)? {
                        value = (value << 8) | *byte as i64;
                    }
                    (len, value.to_string())
                }
                7 => (8, "float".to_string()),
                8 => (0, "0".to_string()),
                9 => (0, "1".to_string()),
                kind if kind >= 12 && kind % 2 == 0 => {
                    let len = ((kind - 12) / 2) as usize;
                    (len, format!("blob({len})"))
                }
                kind if kind >= 13 => {
                    let len = ((kind - 13) / 2) as usize;
                    let text = String::from_utf8_lossy(data.get(body..body + len)?).to_string();
                    (len, text)
                }
                _ => return None,
            };
            body += len;
            out.push(text);
        }
        Some(out)
    }

    fn walk_schema(&self, number: usize, depth: usize) {
        let Some(page) = self.page(number) else {
            println!("  BROKEN: page {number} is outside the file");
            return;
        };
        let header = if number == 1 { 100 } else { 0 };
        let kind = page[header];
        let cells = u16::from_be_bytes([page[header + 3], page[header + 4]]) as usize;
        match kind {
            0x05 => {
                let right = u32::from_be_bytes(page[header + 8..header + 12].try_into().unwrap()) as usize;
                for index in 0..cells {
                    let pointer = header + 12 + index * 2;
                    let cell = u16::from_be_bytes([page[pointer], page[pointer + 1]]) as usize;
                    let child = u32::from_be_bytes(page[cell..cell + 4].try_into().unwrap()) as usize;
                    self.walk_schema(child, depth + 1);
                }
                self.walk_schema(right, depth + 1);
            }
            0x0d => {
                for index in 0..cells {
                    let pointer = header + 8 + index * 2;
                    let cell = u16::from_be_bytes([page[pointer], page[pointer + 1]]) as usize;
                    match self.payload(page, cell).and_then(|(rowid, data)| Some((rowid, self.record(&data)?))) {
                        Some((rowid, columns)) => {
                            let short: Vec<String> = columns
                                .iter()
                                .take(4)
                                .map(|column| column.chars().take(40).collect())
                                .collect();
                            println!("  page {number} row {rowid}: {}", short.join(" | "));
                        }
                        None => println!("  BROKEN: page {number} cell {index} could not be read"),
                    }
                }
            }
            other => println!("  BROKEN: page {number} at depth {depth} has type {other:#04x}, not a table page"),
        }
    }
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).ok_or_else(|| anyhow::anyhow!("usage: wa-dbscan <file.db>"))?;
    let bytes = std::fs::read(&path)?;
    let page_size = u16::from_be_bytes([bytes[16], bytes[17]]) as usize;
    let db = Db { bytes, page_size };
    println!("{} pages of {} bytes", db.pages(), page_size);

    println!("schema:");
    db.walk_schema(1, 0);

    // Look everywhere for table leaf pages that hold schema rows: five columns, the
    // first being "table", "index", "trigger" or "view".
    println!("pages that look like schema leaves:");
    for number in 2..=db.pages() {
        let page = db.page(number).unwrap();
        if page[0] != 0x0d {
            continue;
        }
        let cells = u16::from_be_bytes([page[3], page[4]]) as usize;
        let mut rows = Vec::new();
        for index in 0..cells.min(60) {
            let pointer = 8 + index * 2;
            let cell = u16::from_be_bytes([page[pointer], page[pointer + 1]]) as usize;
            if let Some((rowid, columns)) = db
                .payload(page, cell)
                .and_then(|(rowid, data)| Some((rowid, db.record(&data)?)))
                && columns.len() == 5
                && matches!(columns[0].as_str(), "table" | "index" | "trigger" | "view")
            {
                rows.push(format!("{rowid}:{}:{}:{}", columns[0], columns[1], columns[3]));
            }
        }
        if !rows.is_empty() {
            println!("  page {number}: {}", rows.join("  "));
        }
    }

    let mut kinds: BTreeMap<u8, usize> = BTreeMap::new();
    let mut zero = 0;
    for number in 2..=db.pages() {
        let page = db.page(number).unwrap();
        if page.iter().all(|byte| *byte == 0) {
            zero += 1;
        }
        *kinds.entry(page[0]).or_default() += 1;
    }
    println!("page kinds by first byte (0x0d table leaf, 0x05 table interior, 0x0a index leaf, 0x02 index interior; others are overflow or free):");
    for (kind, count) in kinds {
        println!("  {kind:#04x}: {count}");
    }
    println!("all-zero pages: {zero}");
    // Where the blank pages are, as ranges.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for number in 2..=db.pages() {
        if db.page(number).unwrap().iter().all(|byte| *byte == 0) {
            match ranges.last_mut() {
                Some((_, end)) if *end + 1 == number => *end = number,
                _ => ranges.push((number, number)),
            }
        }
    }
    let text: Vec<String> = ranges
        .iter()
        .map(|(start, end)| if start == end { start.to_string() } else { format!("{start}-{end}") })
        .collect();
    println!("blank page ranges: {}", text.join(", "));
    Ok(())
}
