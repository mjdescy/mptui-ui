mod app;
mod config;
mod publish;

fn main() -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    let result = app::App::default().run(&mut terminal);
    ratatui::restore();
    result
}
