use std::io::{self, Read};

use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    let result = json!({
        "word_count": input.split_whitespace().count(),
        "char_count": input.chars().count(),
        "line_count": input.lines().count(),
    });

    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
