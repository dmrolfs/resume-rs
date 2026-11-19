// ABOUTME: Re-embeds the built-in styles when a file is added to or removed from styles/.
// ABOUTME: include_dir only tracks files it already embedded, so a new file would otherwise be missed.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=styles");
}
