fn main() {
    println!("cargo:rerun-if-changed=assets/PawHide.ico");
    #[cfg(windows)]
    {
        // The exe itself has the approved icon (Explorer, shortcut, taskbar).
        let mut resource = winres::WindowsResource::new();
        resource.set_icon("assets/PawHide.ico");
        resource.set("ProductName", "PawHide");
        resource.set("FileDescription", "PawHide UI");
        resource
            .compile()
            .expect("failed to embed PawHide icon into executable");
    }
}
