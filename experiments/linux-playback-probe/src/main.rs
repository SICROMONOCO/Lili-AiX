#[cfg(target_os = "linux")]
mod linux;

fn main() {
    #[cfg(target_os = "linux")]
    linux::run();
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("Run this probe on Linux. A Windows run cannot validate GTK composition.");
        std::process::exit(1);
    }
}
