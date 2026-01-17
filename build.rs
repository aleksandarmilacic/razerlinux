// Build script for RazerLinux
// Compiles Slint UI files

fn main() {
    // Use cosmic (dark) style for proper dark theme support
    slint_build::compile_with_config(
        "ui/main.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("cosmic".into())
    ).unwrap();
}
