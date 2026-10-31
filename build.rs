fn main() {
    // Embed the manifest (Windows 8+ compatibility, PerMonitorV2 DPI) into the exe.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("app.rc", embed_resource::NONE).manifest_required().unwrap();
    }
}
