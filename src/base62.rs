const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Encodes a u128 number into a base62 string.
///
/// The alphabet used is: `0-9A-Za-z` (62 characters total).
///
/// # Arguments
///
/// * `num` - The number to encode
///
/// # Returns
///
/// A String containing the base62 representation.
///
/// # Examples
///
/// ```
/// use funchain::base62;
///
/// assert_eq!(base62::encode(0), "0");
/// assert_eq!(base62::encode(61), "z");
/// assert_eq!(base62::encode(62), "10");
/// assert_eq!(base62::encode(12345), "3D7");
/// ```
pub fn encode(mut num: u128) -> String {
    if num == 0 {
        return "0".to_string();
    }
    let mut encoded = Vec::new();
    while num > 0 {
        encoded.push(ALPHABET[(num % 62) as usize]);
        num /= 62;
    }
    encoded.reverse();
    String::from_utf8(encoded).unwrap()
}

/// Decodes a base62 string back into a u128 number.
///
/// The alphabet used is: `0-9A-Za-z` (62 characters total).
///
/// # Arguments
///
/// * `s` - The base62 encoded string to decode
///
/// # Returns
///
/// Returns `Ok(u128)` on success, or `Err(String)` if the input contains
/// invalid characters or the result would overflow u128.
///
/// # Examples
///
/// ```
/// use funchain::base62;
///
/// assert_eq!(base62::decode("0").unwrap(), 0);
/// assert_eq!(base62::decode("z").unwrap(), 61);
/// assert_eq!(base62::decode("10").unwrap(), 62);
/// assert_eq!(base62::decode("3D7").unwrap(), 12345);
///
/// // Invalid characters return an error
/// assert!(base62::decode("!").is_err());
///
/// // Empty string returns 0
/// assert_eq!(base62::decode("").unwrap(), 0);
/// ```
///
/// # Roundtrip
///
/// ```
/// use funchain::base62;
///
/// let num = 12345678901234567890u128;
/// assert_eq!(base62::decode(&base62::encode(num)).unwrap(), num);
/// ```
pub fn decode(s: &str) -> Result<u128, String> {
    if s.is_empty() {
        return Ok(0);
    }
    let mut num: u128 = 0;
    for c in s.chars() {
        let val = match c {
            '0'..='9' => c as u128 - '0' as u128,
            'A'..='Z' => c as u128 - 'A' as u128 + 10,
            'a'..='z' => c as u128 - 'a' as u128 + 36,
            _ => return Err(format!("Invalid character: {}", c)),
        };

        match num.checked_mul(62) {
            Some(v) => num = v,
            None => return Err("Overflow: number too large for u128".to_string()),
        }

        match num.checked_add(val) {
            Some(v) => num = v,
            None => return Err("Overflow: number too large for u128".to_string()),
        }
    }
    Ok(num)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ==================== Property-Based Tests ====================

    proptest! {
        /// Any u128 value should roundtrip through encode/decode
        #[test]
        fn prop_roundtrip(n: u128) {
            let encoded = encode(n);
            let decoded = decode(&encoded).unwrap();
            prop_assert_eq!(decoded, n);
        }

        /// Encoded output should only contain valid base62 characters
        #[test]
        fn prop_encode_valid_chars(n: u128) {
            let encoded = encode(n);
            for c in encoded.chars() {
                prop_assert!(
                    c.is_ascii_alphanumeric(),
                    "Invalid character '{}' in encoded output", c
                );
            }
        }

        /// Encoded output should never be empty (at minimum "0")
        #[test]
        fn prop_encode_non_empty(n: u128) {
            let encoded = encode(n);
            prop_assert!(!encoded.is_empty());
        }

        /// Larger numbers should produce longer or equal length encodings
        #[test]
        fn prop_encoding_length_monotonic(a: u128, b: u128) {
            if a < b {
                prop_assert!(encode(a).len() <= encode(b).len());
            }
        }

        /// Valid base62 strings (generated from encode) should always decode successfully
        #[test]
        fn prop_decode_encoded_always_succeeds(n: u128) {
            let encoded = encode(n);
            prop_assert!(decode(&encoded).is_ok());
        }
    }

    // ==================== Unit Tests ====================

    #[test]
    fn test_encode() {
        assert_eq!(encode(0), "0");
        assert_eq!(encode(1), "1");
        assert_eq!(encode(61), "z");
        assert_eq!(encode(62), "10");
        assert_eq!(encode(12345), "3D7");
    }

    #[test]
    fn test_decode() {
        assert_eq!(decode("0").unwrap(), 0);
        assert_eq!(decode("1").unwrap(), 1);
        assert_eq!(decode("z").unwrap(), 61);
        assert_eq!(decode("10").unwrap(), 62);
        assert_eq!(decode("3D7").unwrap(), 12345);
        assert!(decode("!").is_err());
    }

    #[test]
    fn test_roundtrip() {
        let num = 12345678901234567890u128;
        assert_eq!(decode(&encode(num)).unwrap(), num);
    }

    #[test]
    fn test_encode_u128_max() {
        let max = u128::MAX;
        let encoded = encode(max);
        assert_eq!(encoded, "7n42DGM5Tflk9n8mt7Fhc7");
        assert_eq!(decode(&encoded).unwrap(), max);
    }

    #[test]
    fn test_decode_empty_string() {
        assert_eq!(decode("").unwrap(), 0);
    }

    #[test]
    fn test_decode_overflow() {
        // This string represents a number larger than u128::MAX
        let overflow_str = "7n42DGM5Tflk9n8mt7Fhc8"; // one more than max
        assert!(decode(overflow_str).is_err());

        // Very long string that would definitely overflow
        let very_long = "zzzzzzzzzzzzzzzzzzzzzzzzzzzz";
        assert!(decode(very_long).is_err());
    }

    #[test]
    fn test_decode_invalid_characters() {
        assert!(decode("abc!def").is_err());
        assert!(decode("123#456").is_err());
        assert!(decode("hello world").is_err()); // space is invalid
        assert!(decode("-1").is_err()); // negative sign
    }

    #[test]
    fn test_encode_powers_of_62() {
        assert_eq!(encode(1), "1");
        assert_eq!(encode(62), "10");
        assert_eq!(encode(62 * 62), "100");
        assert_eq!(encode(62 * 62 * 62), "1000");
    }

    #[test]
    fn test_roundtrip_various_values() {
        let test_values: Vec<u128> = vec![
            0,
            1,
            61,
            62,
            63,
            100,
            1000,
            10000,
            u128::MAX / 2,
            u128::MAX - 1,
            u128::MAX,
        ];
        for val in test_values {
            assert_eq!(
                decode(&encode(val)).unwrap(),
                val,
                "Failed roundtrip for {}",
                val
            );
        }
    }
}
