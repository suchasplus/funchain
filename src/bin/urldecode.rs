use std::env;
use std::io::{self, Read, Write};
use urlencoding::decode;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && (args[1] == "-h" || args[1] == "--help") {
        println!("Usage: urldecode [STRING]");
        println!("Decodes a URL-encoded string.");
        println!("If no argument is provided, reads from standard input.");
        return Ok(());
    }

    let mut input = String::new();

    if args.len() > 1 {
        // Concatenate arguments with spaces if multiple arguments are provided
        input = args[1..].join(" ");
    } else {
        // Read from stdin
        io::stdin().read_to_string(&mut input)?;
    }

    let decoded = process(&input)?;
    io::stdout().write_all(decoded.as_bytes())?;

    Ok(())
}

fn process(input: &str) -> Result<String, std::string::FromUtf8Error> {
    let decoded = decode(input)?;
    Ok(decoded.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_valid() {
        assert_eq!(process("hello%20world").unwrap(), "hello world");
        assert_eq!(process("foo%2Bbar").unwrap(), "foo+bar");
    }

    #[test]
    fn test_process_no_encoding() {
        assert_eq!(process("hello").unwrap(), "hello");
    }

    #[test]
    fn test_process_invalid_utf8() {
        // Invalid UTF-8 sequence (incomplete surrogate pair)
        let result = process("%FF%FE");
        assert!(result.is_err());
    }

    #[test]
    fn test_process_special_characters() {
        // Test various URL-encoded special characters
        assert_eq!(process("%21").unwrap(), "!"); // exclamation mark
        assert_eq!(process("%40").unwrap(), "@"); // at sign
        assert_eq!(process("%23").unwrap(), "#"); // hash
        assert_eq!(process("%24").unwrap(), "$"); // dollar
        assert_eq!(process("%26").unwrap(), "&"); // ampersand
        assert_eq!(process("%3D").unwrap(), "="); // equals
    }

    #[test]
    fn test_process_unicode() {
        // Chinese character "中" is %E4%B8%AD in UTF-8
        assert_eq!(process("%E4%B8%AD").unwrap(), "中");
        // Emoji "😀" is %F0%9F%98%80 in UTF-8
        assert_eq!(process("%F0%9F%98%80").unwrap(), "😀");
    }

    #[test]
    fn test_process_empty() {
        assert_eq!(process("").unwrap(), "");
    }

    #[test]
    fn test_process_mixed_encoded_and_plain() {
        assert_eq!(process("hello%20world%21").unwrap(), "hello world!");
        assert_eq!(process("a%2Bb%3Dc").unwrap(), "a+b=c");
    }

    #[test]
    fn test_process_plus_sign() {
        // Plus sign is not decoded to space by urlencoding crate (it's form encoding)
        assert_eq!(process("hello+world").unwrap(), "hello+world");
    }
}
