// Embeds the app icon into the Windows .exe (shown in Explorer and the taskbar).
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "EVE BPO Tracker");
        res.set("FileDescription", "EVE BPO Tracker");
        res.set("CompanyName", "Solus Interactive");
        if let Err(e) = res.compile() {
            // Don't fail the build over an icon.
            println!("cargo:warning=Couldn't embed the app icon: {e}");
        }
    }
}
