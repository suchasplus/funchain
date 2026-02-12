use std::env;
use rand::Rng;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && (args[1] == "-h" || args[1] == "--help") {
        println!("Usage: rand32 <LENGTH>");
        println!("Generates a random hex string of the specified byte length.");
        println!("Output length will be 2 * LENGTH.");
        return Ok(());
    }

    if args.len() < 2 {
        eprintln!("Usage: rand32 <length>");
        eprintln!("Try 'rand32 --help' for more information.");
        std::process::exit(1);
    }
    
    // Parse the length argument
    let len: usize = args[1].parse()?;
    
    // Generate hex string
    let hex_string = generate_hex(len);
    println!("{}", hex_string);
    
    Ok(())
}

fn generate_hex(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_hex_length() {
        let len = 10;
        let output = generate_hex(len);
        assert_eq!(output.len(), len * 2);
    }

    #[test]
    fn test_generate_hex_valid_chars() {
        let output = generate_hex(50);
        for c in output.chars() {
            assert!(c.is_digit(16));
        }
    }

    #[test]
    fn test_generate_hex_zero_length() {
        let output = generate_hex(0);
        assert_eq!(output.len(), 0);
        assert_eq!(output, "");
    }

    #[test]
    fn test_generate_hex_one_byte() {
        let output = generate_hex(1);
        assert_eq!(output.len(), 2);
    }

    #[test]
    fn test_generate_hex_randomness() {
        // Generate two outputs and verify they are different (with high probability)
        let output1 = generate_hex(32);
        let output2 = generate_hex(32);
        assert_ne!(output1, output2, "Two random outputs should differ");
    }

    #[test]
    fn test_generate_hex_large_length() {
        let output = generate_hex(1024);
        assert_eq!(output.len(), 2048);
        for c in output.chars() {
            assert!(c.is_digit(16));
        }
    }
}
