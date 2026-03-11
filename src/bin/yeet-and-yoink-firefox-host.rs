fn main() {
    if let Err(err) = yeet_and_yoink::browser_native::run_native_host() {
        eprintln!("yeet-and-yoink-firefox-host: {err:#}");
        std::process::exit(1);
    }
}
