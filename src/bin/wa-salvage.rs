//! Rebuilds the chat database from a damaged copy; see `whatsapp_gpui::salvage`.
//! Usage: wa-salvage <damaged.db> <new.db>

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [damaged, fresh] = args.as_slice() else {
        anyhow::bail!("usage: wa-salvage <damaged.db> <new.db>");
    };
    whatsapp_gpui::salvage::rebuild(damaged, fresh, |line| println!("{line}"))
}
