//! Gives the `coo` helper its own identity: an Info.plist embedded in the
//! binary (`__TEXT,__info_plist`), so macOS names it properly in privacy
//! prompts and logs. A sandboxed build can't launch it without one.

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>il.co.frost-media.pigeon.coo</string>
  <key>CFBundleName</key><string>coo</string>
  <key>CFBundleDisplayName</key><string>Coo command-line tool</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>{version}</string>
</dict>
</plist>
"#
    );
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("Info.plist");
    fs::write(&out, plist).unwrap();
    println!("cargo:rustc-link-arg-bin=coo=-Wl,-sectcreate,__TEXT,__info_plist,{}", out.display());
}
