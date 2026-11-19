fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=src/quill.ico");
    winresource::WindowsResource::new()
        .set_icon("src/quill.ico")
        .set("ProductName", "GlideQuill")
        .set("FileDescription", "GlideQuill")
        .compile()
}
