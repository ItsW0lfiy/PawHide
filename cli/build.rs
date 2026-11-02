fn main() {
    println!("cargo:rerun-if-changed=../ui/assets/PawHide.ico");
    #[cfg(windows)] {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../ui/assets/PawHide.ico");
        res.set("ProductName", "PawHide");
        res.set("FileDescription", "PawHide Command Line Utility");
        res.compile().expect("could not embed PawHide's icon in pawhide.exe");
    }
}
