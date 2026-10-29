//! The string escaper for `expect.json`, which this package writes and a reader reads by eye or with
//! any JSON tool (driver/fuzz/README.md). No dependencies, so no vendoring.

/// Quote `s` as a JSON string.
pub fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn escapes_what_json_requires_and_keeps_the_rest() {
        assert_eq!(quote("a \"q\" \\ b\nc\td\u{1}é"), r#""a \"q\" \\ b\nc\td\u0001é""#);
    }
}
