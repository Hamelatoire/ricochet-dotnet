// Embeds the app icon in the Windows exe as icon resource 1, which is the one GPUI gives the window and taskbar.
fn main() {
    println!("cargo:rerun-if-changed=assets/ricochet.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/ricochet.ico")
            .compile()
            .expect("the icon resource compiles");
    }
}
