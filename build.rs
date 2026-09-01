// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// The Windows version block and application manifest, and why they are filled in at all.
//
// An unsigned binary has nothing but this to identify it. A reputation engine that finds
// a blank publisher, no copyright and no manifest has been handed a file that declines to
// say who wrote it or what it wants, and it scores accordingly — which is half of why a
// fresh dev-prune release reads as unknown-and-therefore-suspect until enough people have
// run it. None of this is a substitute for a signature, and it is not meant to be; it is
// the part that costs nothing and was simply missing.

/// `asInvoker`, stated rather than assumed.
///
/// The MSVC linker embeds no manifest of its own, and the absence of one means the
/// program has never said what privileges it wants. dev-prune wants none: every path runs
/// as the invoking user. The one place elevation would genuinely help — handing a locked
/// file to the session manager during `uninstall` — degrades to a printed command instead
/// of asking for it, which is the whole reason this can be declared honestly.
///
/// It also settles Windows' installer-detection heuristic, which auto-elevates an
/// unmanifested executable whose name looks like a setup program.
#[cfg(target_os = "windows")]
const APP_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");

    // Two different platforms are in play here, and both conditions are needed. The
    // `cfg` is the *host*: a build script is compiled for the machine running the build,
    // and `winres` is only in the dependency graph -- and `rc.exe` only on disk -- when
    // that machine is Windows. `CARGO_CFG_TARGET_OS` is what is being built *for*.
    // Without the second check, cross-building to Linux from a Windows desktop hands the
    // ELF linker a Windows `.res` and fails. CI never sees it, because every target there
    // builds on a runner of its own kind.
    #[cfg(target_os = "windows")]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
            return;
        }

        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set_language(0x0409);

        // ASCII only. winres writes a `.rc` file for `rc.exe`, which reads it in the
        // machine's ANSI codepage unless told otherwise, so a stray em dash here becomes
        // mojibake in the properties dialog on some machines and not others.
        res.set("CompanyName", "VKrishna04");
        res.set("LegalCopyright", "Copyright 2026 VKrishna04. Apache-2.0.");
        res.set(
            "FileDescription",
            "dev-prune - reclaims disk space from idle Git repositories",
        );
        res.set(
            "Comments",
            "https://github.com/Life-Experimentalist/dev-prune",
        );

        // Not `OriginalFilename` or `InternalName`, deliberately. One resource is linked
        // into all three binaries, so whichever name went here would be wrong for two of
        // them — and a file whose `OriginalFilename` disagrees with the file it is in is
        // itself what a scanner treats as a renamed binary.
        res.set_manifest(APP_MANIFEST);

        if let Err(e) = res.compile() {
            // `cargo:warning=`, not `eprintln!`: a build script's stderr is hidden unless
            // the build runs with `-vv`, so the old line meant an icon-less binary
            // shipped without anyone seeing why.
            println!("cargo:warning=Failed to compile the Windows resource: {e}");
        }
    }
}
