fn main() {
    if let Err(error) = factorio_planner_tui::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
