# FunChain CLI Tools

A collection of useful command-line utilities implemented in Rust for everyday tasks.

## 📦 Installation & Build

Ensure you have Rust and Cargo installed.

### Using Make (Recommended)

This project includes a `Makefile` for easy building and installation.

```bash
# Build release binaries
make release

# Install binaries to ~/.local/bin
make install
```

Make sure `~/.local/bin` is in your `PATH`.

### Manual Build

```bash
# Build all tools in release mode
cargo build --release

# Binaries will be located in ./target/release/
ls ./target/release/
```

## 🛠️ Available Commands

All commands support the `--help` flag to display usage information.

| Category | Command | Description |
|----------|---------|-------------|
| **Encoding** | `base64decode` | Decodes Base64 string to raw output. |
| | `urlencode` | Encodes string to URL-safe format. |
| | `urldecode` | Decodes URL-encoded string. |
| **Base62** | `to62` | Converts Decimal (Base10) → Base62. Supports `u128`. |
| | `from62` | Converts Base62 → Decimal (Base10). |
| **Random** | `rand32` | Generates random Hex string (openssl rand -hex). |
| | `rand64` | Generates random Base64 string (openssl rand -base64). |
| **Date/Time** | `strtotime` | Parses natural language date/time to Unix timestamp. |
| **Markdown** | `mt-rs` | Renders Markdown to HTML and opens it in the browser; mirrors the Go [`mt`](https://github.com/suchasplus/madtool) toolchain (single file, multi-file site, live-reload dev server). |

---

### 1. Encoding / Decoding

#### `base64decode`
Decodes a Base64 encoded string.
```bash
echo "aGVsbG8=" | base64decode
# Output: hello
```

#### `urlencode`
Encodes a string into URL-encoded format.
```bash
echo "hello world" | urlencode
# Output: hello%20world
```

#### `urldecode`
Decodes a URL-encoded string.
```bash
echo "hello%20world" | urldecode
# Output: hello world
```

---

### 2. Base62 Number Conversion
Supports large integers (up to `u128`) and uses the standard `0-9A-Za-z` alphabet.

#### `to62` (Decimal → Base62)
```bash
# Argument
to62 12345
# Output: 3D7

# Stdin
echo "12345" | to62
```

#### `from62` (Base62 → Decimal)
```bash
# Argument
from62 3D7
# Output: 12345

# Stdin
echo "3D7" | from62
```

---

### 3. Random String Generation

#### `rand32` (Hex)
Generates random bytes and outputs them as a Hex string.
```bash
# Generate 16 bytes (32 hex chars)
rand32 16
# Output: 4f3a2b1c...
```

#### `rand64` (Base64)
Generates random bytes and outputs them as a Base64 string.
```bash
# Generate 32 bytes (Base64 encoded)
rand64 32
# Output: 2Grln+Qne...
```

---

### 4. Date & Time Parsing

#### `strtotime`
Parses English natural language date/time strings into a Unix timestamp. Inspired by PHP's `strtotime()`.

**Supported formats:**
- **Relative:** `+3 hours`, `2 days ago`, `next friday`, `in 30 minutes`
- **Absolute:** `2025-12-25`, `2026-01-01 12:00`
- **Keywords:** `now`, `tomorrow`, `last day of this month`

```bash
# Relative time
strtotime "+3 hours"
# Output: 1770917276

# Next occurrence of a weekday
strtotime "next friday"
# Output: 1770912000

# Complex phrases
strtotime "last day of this month"
# Output: 1772208000
```

---

### 5. Markdown Renderer

#### `mt-rs`
Renders Markdown to HTML and opens it in your default browser. A Rust port of
the Go [`mt`](https://github.com/suchasplus/madtool) markdown viewer. Bundles
MathJax, Mermaid, Fumadocs-inspired styling, OneNote-flavored mermaid colors,
and an offline syntax highlighter — all embedded into a single binary.

**Three usage modes**

```bash
# 1. One-shot: render to $TMPDIR/mt/<name>.html and open the browser
mt-rs path/to/doc.md

# 2. Self-contained HTML (everything inlined: CSS, JS, fonts, MathJax, Mermaid)
mt-rs -o ~/share-me.html path/to/doc.md

# 3. Directory site mode: every *.md becomes an HTML page with nav, prev/next,
#    cross-file [[Wikilinks]] resolved across the whole tree
mt-rs path/to/docs/
```

**Live-reload dev server**

```bash
# Single file: watches the parent dir to survive editor swap-writes
mt-rs --serve path/to/doc.md

# Directory: watches the tree recursively, rebuilds whole site on any .md change
mt-rs --serve path/to/docs/

# Pick a port (default 7331), or skip the browser launch:
mt-rs --serve --port 8000 --no-open path/to/doc.md
```

**Other useful flags**

```bash
mt-rs --print path/to/doc.md     # render HTML to stdout (good for pipes)
mt-rs --theme dark path/to/...   # force theme (auto | light | dark)
mt-rs --all path/to/docs/        # include files hidden by the config's exclude list
mt-rs --version                  # build metadata (commit + timestamp)
mt-rs --help
```

**Archive for distribution** — `--archive`

```bash
# Zip the rendered site (html + assets + md sources) — extracts into <dirname>/
mt-rs --archive dist/docs.zip path/to/docs/

# HTML only, no markdown sources
mt-rs --no-md --archive dist/docs.zip path/to/docs/

# Cherry-pick files: globs over root-relative paths, repeatable.
# `*` stops at `/`; use `**` to span directories; --exclude wins.
mt-rs --include 'guide/**' --include 'README.md' --exclude '**/wip-*.md' \
      --archive dist/guide.zip path/to/docs/

# Single file → zip with a self-contained HTML + the md source
mt-rs --archive share.zip path/to/doc.md
```

The include/exclude filters shape the build itself, so the nav and
prev/next links inside the archive match exactly what's packed — no dead
links. They also work without `--archive` (plain render and `--serve`).
`--archive` conflicts with `--print`, `-o`, and `--serve`.

**Global config** — `~/.config/mt/config.toml` (honors `$XDG_CONFIG_HOME`)

Directory mode hides AI-assistant prompt files by default so a rendered
site only shows real documentation. Explicit single-file targets
(`mt-rs CLAUDE.md`) always render.

```toml
[site]
# Basenames (case-insensitive) hidden from directory scans.
# Defaults to CLAUDE.md, CLAUDE.local.md, AGENTS.md, GEMINI.md.
# Set to [] to hide nothing; the list REPLACES the default one.
exclude = ["CLAUDE.md", "CLAUDE.local.md", "AGENTS.md", "GEMINI.md"]

# Show each page's source filename as small text under its title in the
# left nav (default true).
nav_filenames = true
```

Pass `--all` to bypass the exclude list for one run without editing the
config.

**Features**

- GitHub-flavored Markdown: tables, strikethrough, autolinks, task lists
- Footnotes, definition lists, typographer, CJK segmentation
- **MathJax** — `$E=mc^2$` inline and `$$ … $$` display
- **Mermaid** — fenced ` ```mermaid ` blocks with OneNote-inspired theme +
  click-to-zoom modal (1:1 / Fit / Auto)
- **Syntax highlighting** via syntect (light + dark themes auto-swap)
- **MkDocs-style admonitions** — `!!! note "Title"` … 4-space indented body
- **Wikilinks** — `[[Page]]` or `[[Page|Alias]]` resolved across the whole
  directory tree
- **YAML frontmatter** — `title`, `description`, `tags`, `theme`
- **Fumadocs-inspired UI** — floating drawers for the left nav and right TOC,
  3-state theme toggle (auto / light / dark) following OS preference
- **Live-reload** via `--serve` (fsnotify + websocket, 120 ms debounce)
- Fully offline: MathJax and Mermaid are baked into the binary

## Development

You can run tests using `cargo test` or `make test`.

```bash
make test           # all tests
make coverage       # cargo-llvm-cov summary
make coverage-html  # HTML report under target/llvm-cov/html/
```

The `mt-rs` static assets (CSS, JS, MathJax, Mermaid) are mirrored from the Go
side via `make sync-mt-assets`. Run that whenever the Go-side `internal/assets/static/`
changes.

## License

MIT
