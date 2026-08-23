fn main() {
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
        if target_env == "msvc" {
            println!("cargo:rustc-link-arg-bin=revy_editor=/STACK:16777216");
        } else {
            println!("cargo:rustc-link-arg-bin=revy_editor=-Wl,--stack,16777216");
        }
    }
}
