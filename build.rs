use std::path::Path;

fn main() {
    // assets/kb 与 assets/skills 不嵌入二进制，由 init.sh 安装到 ~/.oiph
    println!("cargo:rerun-if-changed=assets/kb");
    println!("cargo:rerun-if-changed=assets/skills");
    let _ = Path::new("");
}
