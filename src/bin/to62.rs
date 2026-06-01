use funchain::base62;
use std::env;
use std::io::{self, Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && (args[1] == "-h" || args[1] == "--help") {
        println!("Usage: to62 [NUMBER]");
        println!("Encodes a number to a Base62 string.");
        println!("If no argument is provided, reads from standard input.");
        return Ok(());
    }

    let mut input = String::new();

    if args.len() > 1 {
        // If args are provided, join them.
        input = args[1..].join("");
    } else {
        // Read from stdin.
        io::stdin().read_to_string(&mut input)?;
    }

    match process(&input) {
        Ok(output) => {
            if !output.is_empty() {
                io::stdout().write_all(output.as_bytes())?;
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn process(input: &str) -> Result<String, Box<dyn std::error::Error>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    // Parse input as u128
    let num: u128 = trimmed.parse()?;
    Ok(base62::encode(num))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_valid() {
        assert_eq!(process("0").unwrap(), "0");
        assert_eq!(process("61").unwrap(), "z");
        assert_eq!(process("62").unwrap(), "10");
    }

    #[test]
    fn test_process_empty() {
        assert_eq!(process("").unwrap(), "");
        assert_eq!(process("   ").unwrap(), "");
    }

    #[test]
    fn test_process_invalid() {
        assert!(process("abc").is_err());
        assert!(process("123a").is_err());
    }

    #[test]
    fn test_process_u128_max() {
        // u128::MAX = 340282366920938463463374607431768211455
        let result = process("340282366920938463463374607431768211455").unwrap();
        assert_eq!(result, "7n42DGM5Tflk9n8mt7Fhc7");
    }

    #[test]
    fn test_process_overflow() {
        // u128::MAX + 1 should fail
        assert!(process("340282366920938463463374607431768211456").is_err());

        // Much larger number
        assert!(process("999999999999999999999999999999999999999999").is_err());
    }

    #[test]
    fn test_process_negative() {
        // Negative numbers should fail
        assert!(process("-1").is_err());
        assert!(process("-123").is_err());
    }

    #[test]
    fn test_process_with_whitespace() {
        // Leading/trailing whitespace should be handled
        assert_eq!(process("  123  ").unwrap(), process("123").unwrap());
        assert_eq!(process("\n123\n").unwrap(), process("123").unwrap());
    }
}
