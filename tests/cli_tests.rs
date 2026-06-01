use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

// ============== base64decode tests ==============

#[test]
fn test_base64decode_with_arg() {
    cargo_bin_cmd!("base64decode")
        .arg("aGVsbG8=")
        .assert()
        .success()
        .stdout("hello");
}

#[test]
fn test_base64decode_with_stdin() {
    cargo_bin_cmd!("base64decode")
        .write_stdin("aGVsbG8=")
        .assert()
        .success()
        .stdout("hello");
}

#[test]
fn test_base64decode_invalid() {
    cargo_bin_cmd!("base64decode")
        .arg("not_valid_base64!")
        .assert()
        .failure();
}

// ============== urlencode tests ==============

#[test]
fn test_urlencode_with_arg() {
    cargo_bin_cmd!("urlencode")
        .arg("hello world")
        .assert()
        .success()
        .stdout("hello%20world");
}

#[test]
fn test_urlencode_with_stdin() {
    cargo_bin_cmd!("urlencode")
        .write_stdin("hello world")
        .assert()
        .success()
        .stdout("hello%20world");
}

// ============== urldecode tests ==============

#[test]
fn test_urldecode_with_arg() {
    cargo_bin_cmd!("urldecode")
        .arg("hello%20world")
        .assert()
        .success()
        .stdout("hello world");
}

#[test]
fn test_urldecode_with_stdin() {
    cargo_bin_cmd!("urldecode")
        .write_stdin("hello%20world")
        .assert()
        .success()
        .stdout("hello world");
}

// ============== to62 tests ==============

#[test]
fn test_to62_with_arg() {
    cargo_bin_cmd!("to62")
        .arg("12345")
        .assert()
        .success()
        .stdout("3D7");
}

#[test]
fn test_to62_with_stdin() {
    cargo_bin_cmd!("to62")
        .write_stdin("12345")
        .assert()
        .success()
        .stdout("3D7");
}

#[test]
fn test_to62_invalid() {
    cargo_bin_cmd!("to62")
        .arg("not_a_number")
        .assert()
        .failure();
}

#[test]
fn test_to62_empty() {
    cargo_bin_cmd!("to62")
        .write_stdin("")
        .assert()
        .success()
        .stdout("");
}

// ============== from62 tests ==============

#[test]
fn test_from62_with_arg() {
    cargo_bin_cmd!("from62")
        .arg("3D7")
        .assert()
        .success()
        .stdout("12345");
}

#[test]
fn test_from62_with_stdin() {
    cargo_bin_cmd!("from62")
        .write_stdin("3D7")
        .assert()
        .success()
        .stdout("12345");
}

#[test]
fn test_from62_invalid() {
    cargo_bin_cmd!("from62").arg("invalid!").assert().failure();
}

#[test]
fn test_from62_empty() {
    cargo_bin_cmd!("from62")
        .write_stdin("")
        .assert()
        .success()
        .stdout("");
}

// ============== rand32 tests ==============

#[test]
fn test_rand32_generates_hex() {
    cargo_bin_cmd!("rand32")
        .arg("16")
        .assert()
        .success()
        .stdout(predicate::str::is_match("^[0-9a-f]{32}\n$").unwrap());
}

#[test]
fn test_rand32_no_args() {
    cargo_bin_cmd!("rand32")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

// ============== rand64 tests ==============

#[test]
fn test_rand64_generates_base64() {
    cargo_bin_cmd!("rand64")
        .arg("16")
        .assert()
        .success()
        // Base64 for 16 bytes = ceil(16/3)*4 = 24 chars + newline
        .stdout(predicate::str::is_match("^[A-Za-z0-9+/=]{24}\n$").unwrap());
}

#[test]
fn test_rand64_no_args() {
    cargo_bin_cmd!("rand64")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

// ============== strtotime tests ==============

#[test]
fn test_strtotime_now() {
    cargo_bin_cmd!("strtotime")
        .arg("now")
        .assert()
        .success()
        // Should output a valid timestamp (digits only)
        .stdout(predicate::str::is_match("^\\d+\n$").unwrap());
}

#[test]
fn test_strtotime_relative() {
    cargo_bin_cmd!("strtotime")
        .args(["+1", "hour"])
        .assert()
        .success()
        .stdout(predicate::str::is_match("^\\d+\n$").unwrap());
}

#[test]
fn test_strtotime_invalid() {
    cargo_bin_cmd!("strtotime")
        .arg("invalid_garbage_input")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error"));
}

#[test]
fn test_strtotime_no_args() {
    cargo_bin_cmd!("strtotime")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

// ============== overflow tests ==============

#[test]
fn test_to62_u128_max() {
    // u128::MAX should work
    cargo_bin_cmd!("to62")
        .arg("340282366920938463463374607431768211455")
        .assert()
        .success()
        .stdout("7n42DGM5Tflk9n8mt7Fhc7");
}

#[test]
fn test_to62_overflow() {
    // u128::MAX + 1 should fail
    cargo_bin_cmd!("to62")
        .arg("340282366920938463463374607431768211456")
        .assert()
        .failure();
}

#[test]
fn test_to62_negative() {
    cargo_bin_cmd!("to62").arg("-1").assert().failure();
}

#[test]
fn test_from62_u128_max() {
    // 7n42DGM5Tflk9n8mt7Fhc7 = u128::MAX
    cargo_bin_cmd!("from62")
        .arg("7n42DGM5Tflk9n8mt7Fhc7")
        .assert()
        .success()
        .stdout("340282366920938463463374607431768211455");
}

#[test]
fn test_from62_overflow() {
    // 7n42DGM5Tflk9n8mt7Fhc8 = u128::MAX + 1, should fail
    cargo_bin_cmd!("from62")
        .arg("7n42DGM5Tflk9n8mt7Fhc8")
        .assert()
        .failure();
}

#[test]
fn test_from62_very_long_overflow() {
    // Very long base62 string that would definitely overflow
    cargo_bin_cmd!("from62")
        .arg("zzzzzzzzzzzzzzzzzzzzzzzzzzzzz")
        .assert()
        .failure();
}

// ============== urlencode additional tests ==============

#[test]
fn test_urlencode_unicode() {
    cargo_bin_cmd!("urlencode")
        .arg("中文")
        .assert()
        .success()
        .stdout("%E4%B8%AD%E6%96%87");
}

#[test]
fn test_urlencode_special_chars() {
    cargo_bin_cmd!("urlencode")
        .arg("a=b&c=d")
        .assert()
        .success()
        .stdout("a%3Db%26c%3Dd");
}

// ============== roundtrip tests ==============

#[test]
fn test_base62_roundtrip() {
    // Encode a number
    let encode_output = cargo_bin_cmd!("to62")
        .arg("9876543210")
        .output()
        .expect("Failed to execute to62");
    let encoded = String::from_utf8(encode_output.stdout).unwrap();

    // Decode it back
    cargo_bin_cmd!("from62")
        .arg(encoded.trim())
        .assert()
        .success()
        .stdout("9876543210");
}

#[test]
fn test_url_roundtrip() {
    let original = "hello world & foo=bar";

    // Encode
    let encode_output = cargo_bin_cmd!("urlencode")
        .arg(original)
        .output()
        .expect("Failed to execute urlencode");
    let encoded = String::from_utf8(encode_output.stdout).unwrap();

    // Decode
    cargo_bin_cmd!("urldecode")
        .arg(&encoded)
        .assert()
        .success()
        .stdout(original);
}
