//! Puts Kaye's icon inside the .exe, so Explorer and pinned taskbar icons show it.

fn main() {
    println!("cargo:rerun-if-changed=assets/kaye.ico");
    // Only an .exe has an icon inside it. This script runs on Windows for an Android build
    // too, so it goes by what's being built, not where.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/kaye.ico");
        // A missing resource compiler shouldn't stop Kaye from building. The window
        // still gets its icon at runtime.
        if let Err(error) = resource.compile() {
            println!("cargo:warning=Couldn't embed the .exe icon: {error}");
        }
    }
}
