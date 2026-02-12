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

## Development

You can run tests using `cargo test` or `make test`.

```bash
make test
```

## License

MIT
