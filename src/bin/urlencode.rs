use std::env;
use std::io::{self, Read, Write};
use urlencoding::encode;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && (args[1] == "-h" || args[1] == "--help") {
        println!("Usage: urlencode [STRING]");
        println!("URL-encodes a string.");
        println!("If no argument is provided, reads from standard input.");
        return Ok(());
    }

    let mut input = String::new();

    if args.len() > 1 {
        // Concatenate arguments with spaces if multiple arguments are provided
        input = args[1..].join(" ");
    } else {
        // Read from stdin if no arguments provided
        io::stdin().read_to_string(&mut input)?;
    }

    let encoded = process(&input);
    io::stdout().write_all(encoded.as_bytes())?;

    Ok(())
}

fn process(input: &str) -> String {
    encode(input).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_encode() {
        assert_eq!(process("hello world"), "hello%20world");
        assert_eq!(process("foo+bar"), "foo%2Bbar");
    }

    #[test]
    fn test_process_no_change() {
        assert_eq!(process("abc"), "abc");
    }

    #[test]
    fn test_process_empty() {
        assert_eq!(process(""), "");
    }

    #[test]
    fn test_process_unicode() {
        // Chinese character "中" -> %E4%B8%AD
        assert_eq!(process("中"), "%E4%B8%AD");
        // Emoji "😀" -> %F0%9F%98%80
        assert_eq!(process("😀"), "%F0%9F%98%80");
        // Mixed
        assert_eq!(process("hello中文"), "hello%E4%B8%AD%E6%96%87");
    }

    #[test]
    fn test_process_special_characters() {
        assert_eq!(process("!"), "%21");
        assert_eq!(process("@"), "%40");
        assert_eq!(process("#"), "%23");
        assert_eq!(process("$"), "%24");
        assert_eq!(process("&"), "%26");
        assert_eq!(process("="), "%3D");
        assert_eq!(process("?"), "%3F");
        assert_eq!(process("/"), "%2F");
    }

    #[test]
    fn test_process_url_safe_chars() {
        // These characters should NOT be encoded
        assert_eq!(process("abc"), "abc");
        assert_eq!(process("ABC"), "ABC");
        assert_eq!(process("123"), "123");
        assert_eq!(process("-_.~"), "-_.~");
    }

    #[test]
    fn test_process_mixed() {
        assert_eq!(process("key=value&foo=bar"), "key%3Dvalue%26foo%3Dbar");
        assert_eq!(process("hello world!"), "hello%20world%21");
        assert_eq!(process("path/to/file"), "path%2Fto%2Ffile");
    }

    #[test]
    fn test_process_newlines() {
        assert_eq!(process("\n"), "%0A");
        assert_eq!(process("\r\n"), "%0D%0A");
        assert_eq!(process("line1\nline2"), "line1%0Aline2");
    }
}
