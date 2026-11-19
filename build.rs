fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=src/quill.ico");
    println!("cargo:rerun-if-changed=src/library.rc");
    winresource::WindowsResource::new()
        .append_rc_content(include_str!("src/library.rc"))
        .set_icon("src/quill.ico")
        .set("ProductName", "GlideQuill")
        .set("FileDescription", "GlideQuill")
        .compile()
}
