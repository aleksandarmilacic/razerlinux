// Build script for RazerLinux
// Compiles Slint UI files

fn main() {
    slint_build::compile("ui/main.slint").unwrap();
}
