//! Embeds the app icon in the Windows executables.

fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/app.ico");
        if let Err(error) = resource.compile() {
            println!("cargo:warning=could not embed the app icon: {error}");
        }
    }
}
