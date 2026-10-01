//! Token format: "tell_" followed by 32 random bytes in hex.

const PREFIX: &str = "tell_";

pub fn generate() -> String {
    let bytes: [u8; 32] = rand::random();
    let mut token = String::with_capacity(PREFIX.len() + 2 * bytes.len());
    token.push_str(PREFIX);
    for byte in bytes {
        token.push_str(&format!("{byte:02x}"));
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_and_64_hex_digits() {
        let token = generate();
        let hex = token.strip_prefix(PREFIX).unwrap();
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn tokens_differ() {
        assert_ne!(generate(), generate());
    }
}
