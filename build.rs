fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");

    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set_language(0x0409);
        if let Err(e) = res.compile() {
            // `cargo:warning=`, not `eprintln!`: a build script's stderr is hidden unless
            // the build runs with `-vv`, so the old line meant an icon-less binary
            // shipped without anyone seeing why.
            println!("cargo:warning=Failed to compile the Windows resource icon: {e}");
        }
    }
}
