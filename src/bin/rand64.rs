use std::env;
use rand::Rng;
use base64::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && (args[1] == "-h" || args[1] == "--help") {
        println!("Usage: rand64 <LENGTH>");
        println!("Generates a random Base64 string of the specified byte length.");
        return Ok(());
    }

    if args.len() < 2 {
        eprintln!("Usage: rand64 <length>");
        eprintln!("Try 'rand64 --help' for more information.");
        std::process::exit(1);
    }
    
    // Parse the length argument
    let len: usize = args[1].parse()?;
    
    let encoded = generate_base64(len);
    println!("{}", encoded);
    
    Ok(())
}

fn generate_base64(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    rand::rng().fill_bytes(&mut bytes);
    BASE64_STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_base64_length() {
        // Base64 length formula: 4 * ceil(n / 3)
        assert_eq!(generate_base64(1).len(), 4);
        assert_eq!(generate_base64(2).len(), 4);
        assert_eq!(generate_base64(3).len(), 4);
        assert_eq!(generate_base64(4).len(), 8);
    }

    #[test]
    fn test_generate_base64_validity() {
        let output = generate_base64(100);
        assert!(BASE64_STANDARD.decode(&output).is_ok());
    }

    #[test]
    fn test_generate_base64_zero_length() {
        let output = generate_base64(0);
        assert_eq!(output.len(), 0);
        assert_eq!(output, "");
    }

    #[test]
    fn test_generate_base64_one_byte() {
        let output = generate_base64(1);
        assert_eq!(output.len(), 4); // Base64 pads to 4 chars
        assert!(BASE64_STANDARD.decode(&output).is_ok());
    }

    #[test]
    fn test_generate_base64_randomness() {
        // Generate two outputs and verify they are different (with high probability)
        let output1 = generate_base64(32);
        let output2 = generate_base64(32);
        assert_ne!(output1, output2, "Two random outputs should differ");
    }

    #[test]
    fn test_generate_base64_large_length() {
        let output = generate_base64(1024);
        // Base64 length for 1024 bytes: ceil(1024/3) * 4 = 1368
        assert_eq!(output.len(), 1368);
        assert!(BASE64_STANDARD.decode(&output).is_ok());
    }

    #[test]
    fn test_generate_base64_decode_roundtrip() {
        let output = generate_base64(50);
        let decoded = BASE64_STANDARD.decode(&output).unwrap();
        assert_eq!(decoded.len(), 50);
    }
}
