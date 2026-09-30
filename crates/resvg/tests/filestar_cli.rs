// Copyright 2026 the Resvg Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Filestar's patches to the resvg CLI: it reads what the 0.35.1 build (filestar/resvg) read,
//! forgiving by default, and `--unforgiving` restores upstream's behaviour.

use std::path::PathBuf;
use std::process::Command;

const RECT: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'><rect width='5' height='5'/></svg>";

fn render(name: &str, svg: &[u8], extra: &[&str]) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("resvg-filestar-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input: PathBuf = dir.join(format!("{name}.svg"));
    let output: PathBuf = dir.join(format!("{name}.png"));
    std::fs::write(&input, svg).unwrap();
    let _ = std::fs::remove_file(&output);
    let result = Command::new(env!("CARGO_BIN_EXE_resvg"))
        .args(extra)
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    (
        result.status.success() && output.exists(),
        String::from_utf8_lossy(&result.stderr).into_owned(),
    )
}

#[test]
fn utf16_input() {
    let mut le = vec![0xFF, 0xFE];
    RECT.encode_utf16().for_each(|u| le.extend(u.to_le_bytes()));
    assert!(render("utf16le", &le, &[]).0);

    let mut be = vec![0xFE, 0xFF];
    RECT.encode_utf16().for_each(|u| be.extend(u.to_be_bytes()));
    assert!(render("utf16be", &be, &[]).0);
}

#[test]
fn nul_characters() {
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'>\0<rect width='5' height='5'/></svg>";
    assert!(render("nul", svg, &[]).0);
    let (ok, stderr) = render("nul-strict", svg, &["--unforgiving"]);
    assert!(!ok && stderr.contains("non-XML character"), "{stderr}");
}

#[test]
fn unreadable_attribute() {
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'><rect id='a&b' width='5' height='5'/></svg>";
    assert!(render("entity", svg, &[]).0);
    let (ok, stderr) = render("entity-strict", svg, &["--unforgiving"]);
    assert!(!ok && stderr.contains("malformed entity reference"), "{stderr}");
}

#[test]
fn invalid_size() {
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg' width='0' height='0'><rect width='5' height='5'/></svg>";
    assert!(render("size", svg, &[]).0);
    let (ok, stderr) = render("size-strict", svg, &["--unforgiving"]);
    assert!(!ok && stderr.contains("invalid size"), "{stderr}");
}
