//! `--archive` — zip packaging of rendered output for distribution.
//!
//! Layout inside the zip: everything sits under a single top-level folder
//! (the site slug), so extraction never splats files into the current
//! directory:
//!
//! ```text
//! <root_name>/README.html
//! <root_name>/guide/intro.html
//! <root_name>/assets/style.css
//! <root_name>/README.md            (unless --no-md)
//! <root_name>/guide/intro.md
//! ```

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

#[derive(Debug)]
pub enum PackError {
    Io(std::io::Error),
    Zip(String),
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackError::Io(e) => write!(f, "io: {e}"),
            PackError::Zip(e) => write!(f, "zip: {e}"),
        }
    }
}

impl std::error::Error for PackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PackError::Io(e) => Some(e),
            PackError::Zip(_) => None,
        }
    }
}

impl From<std::io::Error> for PackError {
    fn from(e: std::io::Error) -> Self {
        PackError::Io(e)
    }
}

/// Writes `out_zip` containing the whole `dir` tree plus `extra_files`
/// (absolute source path → zip-relative slash path), all nested under
/// `root_name/`. Returns the number of entries written. Overwrites an
/// existing archive; creates parent directories of `out_zip` as needed.
pub fn pack(
    out_zip: &Path,
    root_name: &str,
    dir: &Path,
    extra_files: &[(std::path::PathBuf, String)],
) -> Result<usize, PackError> {
    if let Some(parent) = out_zip.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::create(out_zip)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut count = 0usize;
    for entry in walk_dir_files(dir) {
        let rel = entry
            .path()
            .strip_prefix(dir)
            .unwrap_or(entry.path())
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect::<Vec<_>>()
            .join("/");
        add_file(&mut zip, opts, entry.path(), &format!("{root_name}/{rel}"))?;
        count += 1;
    }
    for (src, rel) in extra_files {
        add_file(&mut zip, opts, src, &format!("{root_name}/{rel}"))?;
        count += 1;
    }
    zip.finish().map_err(|e| PackError::Zip(e.to_string()))?;
    Ok(count)
}

fn add_file(
    zip: &mut zip::ZipWriter<File>,
    opts: SimpleFileOptions,
    src: &Path,
    name_in_zip: &str,
) -> Result<(), PackError> {
    zip.start_file(name_in_zip, opts)
        .map_err(|e| PackError::Zip(e.to_string()))?;
    let mut f = File::open(src)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    zip.write_all(&buf)?;
    Ok(())
}

fn walk_dir_files(dir: &Path) -> impl Iterator<Item = walkdir::DirEntry> {
    WalkDir::new(dir)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tmp(label: &str) -> PathBuf {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!("mt-pack-{label}-{pid}-{nanos}"));
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn write(path: PathBuf, content: &str) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, content).unwrap();
    }
    fn zip_names(path: &Path) -> Vec<String> {
        let f = File::open(path).unwrap();
        let mut ar = zip::ZipArchive::new(f).unwrap();
        (0..ar.len())
            .map(|i| ar.by_index(i).unwrap().name().to_string())
            .collect()
    }

    #[test]
    fn pack_nests_site_tree_and_md_under_root_name() {
        let site = tmp("site");
        write(site.join("README.html"), "<html>home</html>");
        write(site.join("guide/intro.html"), "<html>intro</html>");
        write(site.join("assets/style.css"), "body{}");
        let srcs = tmp("srcs");
        write(srcs.join("README.md"), "# home");
        write(srcs.join("guide/intro.md"), "# intro");

        let out = tmp("out").join("dist/docs.zip");
        let extra = vec![
            (srcs.join("README.md"), "README.md".to_string()),
            (srcs.join("guide/intro.md"), "guide/intro.md".to_string()),
        ];
        let n = pack(&out, "docs", &site, &extra).unwrap();

        let mut names = zip_names(&out);
        names.sort();
        assert_eq!(
            names,
            vec![
                "docs/README.html",
                "docs/README.md",
                "docs/assets/style.css",
                "docs/guide/intro.html",
                "docs/guide/intro.md",
            ],
            "zip layout wrong"
        );
        assert_eq!(n, 5, "entry count");
    }

    #[test]
    fn pack_without_extra_files_holds_html_only() {
        let site = tmp("site2");
        write(site.join("a.html"), "<html>a</html>");
        let out = tmp("out2").join("a.zip");
        let n = pack(&out, "a", &site, &[]).unwrap();
        assert_eq!(zip_names(&out), vec!["a/a.html"]);
        assert_eq!(n, 1);
    }

    #[test]
    fn pack_roundtrip_preserves_content_and_utf8_names() {
        let site = tmp("site3");
        write(site.join("测试.html"), "<html>中文</html>");
        let out = tmp("out3").join("x.zip");
        pack(&out, "站点", &site, &[]).unwrap();

        let f = File::open(&out).unwrap();
        let mut ar = zip::ZipArchive::new(f).unwrap();
        let mut entry = ar.by_name("站点/测试.html").expect("utf8 name lookup");
        let mut s = String::new();
        entry.read_to_string(&mut s).unwrap();
        assert_eq!(s, "<html>中文</html>");
    }

    #[test]
    fn pack_overwrites_existing_archive() {
        let site = tmp("site4");
        write(site.join("one.html"), "1");
        let out = tmp("out4").join("x.zip");
        pack(&out, "r", &site, &[]).unwrap();
        write(site.join("two.html"), "2");
        let n = pack(&out, "r", &site, &[]).unwrap();
        assert_eq!(n, 2, "second pack should see both files");
        assert_eq!(zip_names(&out).len(), 2);
    }
}
