use std::env;
use std::io::{self, Read, Write};
use funchain::base62;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && (args[1] == "-h" || args[1] == "--help") {
        println!("Usage: from62 [STRING]");
        println!("Decodes a Base62 encoded string to a number.");
        println!("If no argument is provided, reads from standard input.");
        return Ok(());
    }

    let mut input = String::new();

    if args.len() > 1 {
        input = args[1..].join("");
    } else {
        io::stdin().read_to_string(&mut input)?;
    }

    match process(&input) {
        Ok(output) => {
            if !output.is_empty() {
                io::stdout().write_all(output.as_bytes())?;
            }
            Ok(())
        },
        Err(e) => Err(e.into()),
    }
}

fn process(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let decoded = base62::decode(trimmed)?;
    Ok(decoded.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_valid() {
        assert_eq!(process("z").unwrap(), "61");
        assert_eq!(process("10").unwrap(), "62");
    }

    #[test]
    fn test_process_empty() {
        assert_eq!(process("").unwrap(), "");
        assert_eq!(process("   ").unwrap(), "");
    }

    #[test]
    fn test_process_invalid() {
        assert!(process("!").is_err());
    }

    #[test]
    fn test_process_u128_max() {
        // 7n42DGM5Tflk9n8mt7Fhc7 = u128::MAX
        let result = process("7n42DGM5Tflk9n8mt7Fhc7").unwrap();
        assert_eq!(result, "340282366920938463463374607431768211455");
    }

    #[test]
    fn test_process_overflow() {
        // 7n42DGM5Tflk9n8mt7Fhc8 = u128::MAX + 1, should fail
        assert!(process("7n42DGM5Tflk9n8mt7Fhc8").is_err());

        // Very long base62 string that would overflow
        assert!(process("zzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
    }

    #[test]
    fn test_process_invalid_characters() {
        assert!(process("abc!def").is_err());
        assert!(process("hello world").is_err()); // space is invalid
        assert!(process("123#456").is_err());
    }

    #[test]
    fn test_process_with_whitespace() {
        // Leading/trailing whitespace should be handled
        assert_eq!(process("  z  ").unwrap(), process("z").unwrap());
        assert_eq!(process("\n10\n").unwrap(), process("10").unwrap());
    }

    #[test]
    fn test_process_zero() {
        assert_eq!(process("0").unwrap(), "0");
    }
}
