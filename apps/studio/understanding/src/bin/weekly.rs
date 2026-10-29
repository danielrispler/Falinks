//! The weekly check on the questions (#328, module 6): prints the last 7 days' thinking, surprise and
//! `unknown` rates from the card store. Words after the command are Daniel's one-line verdict, which
//! it appends with the rates to `weekly.txt` beside the store.

use std::{fs::OpenOptions, io::Write, time::UNIX_EPOCH};
use studio_understanding::{CardStore, week};

fn main() -> std::io::Result<()> {
    let log = studio_hook::event_log(&std::env::home_dir().expect("a home directory"));
    // The app keeps the store beside the log.
    let store = CardStore::open(log.with_file_name("cards.json"))?;
    let now = UNIX_EPOCH.elapsed().unwrap_or_default().as_millis() as u64;
    let rates = week(store.cards(), now);
    print!("{rates}");
    let verdict = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if !verdict.is_empty() {
        let weekly = log.with_file_name("weekly.txt");
        let mut file = OpenOptions::new().create(true).append(true).open(&weekly)?;
        writeln!(file, "{rates}verdict: {verdict}\n")?;
        println!("Kept your verdict in {}", weekly.display());
    }
    Ok(())
}
