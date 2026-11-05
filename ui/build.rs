fn main() {
    println!("cargo:rerun-if-changed=src/pawhide.slint");
    slint_build::compile("src/pawhide.slint")
        .expect("Slint interface failed to compile");

    println!("cargo:rerun-if-changed=assets/PawHide.ico");
    #[cfg(windows)]
    {
        let mut resource = winres::WindowsResource::new();
        resource.set_icon("assets/PawHide.ico");
        resource.set("ProductName", "PawHide");
        resource.set("FileDescription", "PawHide UI — Slint");
        resource.compile().expect("could not embed PawHide icon");
    }
}
