use std::io::{self, Read};

/// Host and manifest enforce this same bounded stdin contract.
const MAX_INPUT_BYTES: usize = 64 * 1024;

struct Counts {
    word_count: usize,
    char_count: usize,
    line_count: usize,
}

fn read_bounded(mut reader: impl Read) -> io::Result<String> {
    let mut bytes = Vec::with_capacity(MAX_INPUT_BYTES);
    reader
        .by_ref()
        .take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input exceeds the 65536-byte capability limit",
        ));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn validate_input(input: &str) -> io::Result<()> {
    if input.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input must contain at least one non-whitespace character",
        ));
    }
    Ok(())
}

fn count(input: &str) -> Counts {
    Counts {
        word_count: input.split_whitespace().count(),
        char_count: input.chars().count(),
        line_count: input.lines().count(),
    }
}

/// Counts are non-negative integers, so fixed-field JSON needs no escaping.
fn render_json(counts: &Counts) -> String {
    format!(
        "{{\"word_count\":{},\"char_count\":{},\"line_count\":{}}}",
        counts.word_count, counts.char_count, counts.line_count
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = read_bounded(io::stdin())?;
    validate_input(&input)?;
    println!("{}", render_json(&count(&input)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_unicode_and_lines_deterministically() {
        let output = render_json(&count("hello\nλ world"));
        assert_eq!(output, r#"{"word_count":3,"char_count":13,"line_count":2}"#);
    }

    #[test]
    fn result_stays_within_declared_output_bound() {
        let input = "x".repeat(MAX_INPUT_BYTES);
        let output = render_json(&count(&input));
        assert!(output.len() <= 128);
    }

    #[test]
    fn rejects_overlong_non_utf8_and_blank_input() {
        assert!(read_bounded("x".repeat(MAX_INPUT_BYTES + 1).as_bytes()).is_err());
        assert!(read_bounded([0xff].as_slice()).is_err());
        assert!(validate_input(" \n\t").is_err());
    }
}
