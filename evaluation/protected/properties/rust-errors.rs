//! Differential properties R1-R4 (#228 decisions 10-11; evaluation/code-quality.json). Each
//! property prints one line per seeded input; `quality` compares the lines with the frozen
//! reference's. Only the API the oracle imports is used.
use catalog::catalog::lookup;
use catalog::consumer::{describe, total};
use std::panic::{UnwindSafe, catch_unwind};

const SEED: u64 = 228;
const INPUTS: usize = 500;

/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    fn n(&mut self, k: usize) -> usize {
        (self.next() % k as u64) as usize
    }

    /// ASCII letters, digits, space, tab and '=', length 0-12.
    fn generated(&mut self) -> String {
        const CHARS: &[u8] =
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 \t=";
        (0..self.n(13))
            .map(|_| CHARS[self.n(CHARS.len())] as char)
            .collect()
    }

    /// apple, pear, offline or a generated key.
    fn key(&mut self) -> String {
        match self.n(4) {
            0 => "apple".into(),
            1 => "pear".into(),
            2 => "offline".into(),
            _ => self.generated(),
        }
    }

    /// offline or a generated missing key.
    fn error_key(&mut self) -> String {
        if self.n(2) == 0 {
            "offline".into()
        } else {
            format!("x{}", self.generated())
        }
    }
}

fn emit(id: &str, i: usize, value: impl FnOnce() -> String + UnwindSafe) {
    match catch_unwind(value) {
        Ok(value) => println!("falinks-property {id} {i} {value:?}"),
        Err(_) => println!("falinks-property {id} {i} panic"),
    }
}

fn sum(keys: Vec<String>) -> String {
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    format!("{:?}", total(&keys))
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let properties: [(&str, fn(&mut Rng) -> Box<dyn FnOnce() -> String + UnwindSafe>); 4] = [
        ("R1", |r| {
            let key = r.key();
            Box::new(move || format!("{:?}", lookup(&key)))
        }),
        ("R2", |r| {
            let keys: Vec<String> = (0..r.n(9))
                .map(|_| if r.n(2) == 0 { "apple" } else { "pear" }.to_string())
                .collect();
            Box::new(move || sum(keys))
        }),
        ("R3", |r| {
            let mut keys: Vec<String> = (0..1 + r.n(8))
                .map(|_| match r.n(4) {
                    0 => "apple".into(),
                    1 => "pear".into(),
                    _ => r.error_key(),
                })
                .collect();
            let at = r.n(keys.len());
            keys[at] = r.error_key();
            Box::new(move || sum(keys))
        }),
        ("R4", |r| {
            let key = r.key();
            Box::new(move || describe(&key))
        }),
    ];
    for (id, input) in properties {
        let mut r = Rng(SEED);
        for i in 0..INPUTS {
            emit(id, i, input(&mut r));
        }
    }
}
