//! Embeds the app icon into converty.exe on Windows.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows/converty.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/windows/converty.ico");
        res.set("ProductName", "Converty");
        res.set("FileDescription", "Converty PDF toolbox");
        if let Err(e) = res.compile() {
            // Cross builds without a resource compiler still work, just without the icon.
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
