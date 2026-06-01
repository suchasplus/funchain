//! Cross-platform browser launcher. Wraps the `webbrowser` crate so tests can
//! inject a stub via [`Opener::custom`].

use std::path::Path;

pub trait Opener {
    fn open(&self, target: &str) -> Result<(), Box<dyn std::error::Error>>;
}

/// Real opener — delegates to the OS default handler via the `webbrowser` crate.
pub struct Default;

impl Opener for Default {
    fn open(&self, target: &str) -> Result<(), Box<dyn std::error::Error>> {
        webbrowser::open(target).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }
}

/// Convenience: open a filesystem path as `file://` URL.
pub fn open_file(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let s = path.to_string_lossy().into_owned();
    Default.open(&s)
}

/// Convenience: open an arbitrary URL (http://, https://, etc.).
pub fn open_url(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    Default.open(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Spy {
        seen: RefCell<Vec<String>>,
    }
    impl Opener for Spy {
        fn open(&self, target: &str) -> Result<(), Box<dyn std::error::Error>> {
            self.seen.borrow_mut().push(target.to_string());
            Ok(())
        }
    }

    #[test]
    fn spy_records_target() {
        let spy = Spy {
            seen: RefCell::new(vec![]),
        };
        spy.open("https://example.com").unwrap();
        assert_eq!(
            spy.seen.borrow().as_slice(),
            &["https://example.com".to_string()]
        );
    }
}
