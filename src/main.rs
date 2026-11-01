#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
mod selftest;
#[cfg(windows)]
mod settings_window;
#[cfg(windows)]
mod win;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--selftest") {
        std::process::exit(selftest::run(&args[1..]));
    }

    #[cfg(windows)]
    app::run();

    #[cfg(not(windows))]
    {
        eprintln!("Графический режим работает только в Windows.\nПроверка распознавания: HoverCopy --selftest IMAGE X Y [DPI]");
        std::process::exit(2);
    }
}
