fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("sunlu.ico");
        if let Err(err) = res.compile() {
            panic!("Windows-Symbol konnte nicht eingebunden werden: {err}");
        }
    }
}
