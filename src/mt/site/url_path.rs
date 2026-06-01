//! Boundary between site-internal slash paths and URL-encoded paths.
//!
//! Internal representation
//!   `Entry.rel`, `Entry.out_rel`, and every other slash-separated path the
//!   site model passes around store **decoded** characters as they exist on
//!   disk (`"guide/My Page.html"`, `"你好.html"`).
//!
//! Encoding at the boundary
//!   Anything that becomes an HTML `href` or shows up in a browser request
//!   must be percent-encoded *per segment* — `/` separators stay literal, but
//!   spaces / non-ASCII / reserved characters in each segment get encoded.
//!   That way the browser's GET arrives as a well-formed URL and the server
//!   can decode it back to the on-disk name before the lookup.

use std::borrow::Cow;

/// Percent-encodes each `/`-separated segment of `path` independently,
/// leaving the slashes themselves untouched. Designed for slash-joined
/// site-relative paths like `guide/My Page.html` → `guide/My%20Page.html`.
pub fn encode_segments(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut first = true;
    for seg in path.split('/') {
        if !first {
            out.push('/');
        }
        first = false;
        out.push_str(&urlencoding::encode(seg));
    }
    out
}

/// Percent-decodes a slash-separated URL path back to its on-disk form. The
/// HTTP server applies this to the incoming request before looking the page
/// up in the in-memory map (which is keyed by the *decoded* `out_rel`).
///
/// On malformed UTF-8 we fall back to the raw input — better to 404 than to
/// panic on a malicious or garbled request.
pub fn decode(url_path: &str) -> Cow<'_, str> {
    match urlencoding::decode(url_path) {
        Ok(s) => s,
        Err(_) => Cow::Borrowed(url_path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_each_segment_independently() {
        assert_eq!(
            encode_segments("guide/My Page.html"),
            "guide/My%20Page.html"
        );
        assert_eq!(encode_segments("你好.html"), "%E4%BD%A0%E5%A5%BD.html");
        // Multiple levels.
        assert_eq!(
            encode_segments("a b/c d/e f.html"),
            "a%20b/c%20d/e%20f.html"
        );
    }

    #[test]
    fn slash_is_never_encoded() {
        let out = encode_segments("a/b/c");
        assert!(out.contains('/'));
        assert!(!out.contains("%2F"));
    }

    #[test]
    fn unreserved_ascii_passes_through() {
        // RFC 3986 unreserved: A-Z a-z 0-9 - _ . ~
        let s = "README.html";
        assert_eq!(encode_segments(s), s);
        // Dot-segments and dashes survive untouched.
        assert_eq!(encode_segments("guide/api-v2.html"), "guide/api-v2.html");
    }

    #[test]
    fn encode_then_decode_roundtrips() {
        for s in [
            "guide/My Page.html",
            "你好.html",
            "tricky & special!.html",
            "guide/sub dir/leaf.html",
        ] {
            let enc = encode_segments(s);
            let dec = decode(&enc);
            assert_eq!(dec.as_ref(), s, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn decode_handles_unencoded_input() {
        // Server may see a request that's already plain ASCII — decode is a no-op.
        assert_eq!(decode("README.html"), "README.html");
    }
}
