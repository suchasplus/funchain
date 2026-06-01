use base64::prelude::*;
use std::env;
use std::io::{self, Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && (args[1] == "-h" || args[1] == "--help") {
        println!("Usage: base64decode [STRING]");
        println!("Decodes a Base64 encoded string.");
        println!("If no argument is provided, reads from standard input.");
        return Ok(());
    }

    let mut input = String::new();

    if args.len() > 1 {
        // Read from arguments
        input = args[1..].join(" ");
    } else {
        // Read from stdin
        io::stdin().read_to_string(&mut input)?;
    }

    let decoded = process(&input)?;
    io::stdout().write_all(&decoded)?;

    Ok(())
}

fn clean_input(input: &str) -> String {
    input.chars().filter(|c| !c.is_whitespace()).collect()
}

fn process(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let clean = clean_input(input);
    BASE64_STANDARD.decode(clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_input() {
        assert_eq!(clean_input("aGVsbG8=\n"), "aGVsbG8=");
        assert_eq!(clean_input(" aG Vsb G8= "), "aGVsbG8=");
        assert_eq!(clean_input("aGVsbG8=\r\n"), "aGVsbG8=");
    }

    #[test]
    fn test_process() {
        assert_eq!(process("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(process(" aGVsbG8= ").unwrap(), b"hello");
        // Test invalid base64
        assert!(process("not base64!").is_err());
    }

    #[test]
    fn test_process_invalid_base64_various() {
        // Invalid characters
        assert!(process("abc!def").is_err());
        assert!(process("abc@def").is_err());
        assert!(process("abc#def").is_err());

        // Invalid length (not multiple of 4 without proper padding)
        assert!(process("abc").is_err());

        // Invalid padding position
        assert!(process("=abc").is_err());
        assert!(process("a=bc").is_err());
    }

    #[test]
    fn test_process_valid_various() {
        // Empty string encodes to empty
        assert_eq!(process("").unwrap(), b"");

        // Single character (needs padding)
        assert_eq!(process("YQ==").unwrap(), b"a");

        // Two characters
        assert_eq!(process("YWI=").unwrap(), b"ab");

        // Three characters (no padding needed)
        assert_eq!(process("YWJj").unwrap(), b"abc");

        // Longer string
        assert_eq!(process("SGVsbG8gV29ybGQh").unwrap(), b"Hello World!");
    }

    #[test]
    fn test_process_with_whitespace() {
        // Multiline base64 (common in PEM files)
        let multiline = "SGVs\nbG8g\nV29y\nbGQh";
        assert_eq!(process(multiline).unwrap(), b"Hello World!");

        // Tabs and spaces
        let with_tabs = "SGVs\tbG8g\tV29y\tbGQh";
        assert_eq!(process(with_tabs).unwrap(), b"Hello World!");
    }

    #[test]
    fn test_process_binary_data() {
        // Binary data with null bytes
        assert_eq!(process("AAAA").unwrap(), vec![0, 0, 0]);

        // All 0xFF bytes
        assert_eq!(process("////").unwrap(), vec![0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_clean_input_preserves_valid_chars() {
        // Only whitespace should be removed
        assert_eq!(clean_input("YWJj"), "YWJj");
        assert_eq!(clean_input("+/=="), "+/==");
    }
}
