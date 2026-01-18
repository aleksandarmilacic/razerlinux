// Build script for RazerLinux
// Compiles Slint UI files and exports version information

fn main() {
    // Export version from Cargo.toml as environment variable for use in code
    let version = env!("CARGO_PKG_VERSION");
    println!("cargo:rustc-env=RAZERLINUX_VERSION={}", version);
    
    // Use cosmic (dark) style for proper dark theme support
    slint_build::compile_with_config(
        "ui/main.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("cosmic".into())
    ).unwrap();
}
