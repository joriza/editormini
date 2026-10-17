// example-code.rs — Rust sample
use std::collections::HashMap;

fn main() {
    let mut scores: HashMap<&str, i32> = HashMap::new();

    scores.insert("dragon", 100);
    scores.insert("wizard", 200);
    scores.insert("rogue", 150);

    for (class, score) in &scores {
        println!("{}: {}", class, score);
    }

    let result = match scores.get("dragon") {
        Some(v) => v,
        None => &0,
    };

    println!("Dragon score: {}", result);
}
