mod app;
mod config;
mod publish;
mod setup;

fn main() -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

/// Show the first-run setup wizard when no config file exists, then load
/// the main UI. `App::default()` reads the config file, so a file written
/// by the wizard is picked up before the main UI appears.
fn run(terminal: &mut ratatui::DefaultTerminal) -> std::io::Result<()> {
    if let Some(path) = config::config_file_path()
        && !path.exists()
        && setup::run(terminal, &path)? == setup::SetupOutcome::Exited
    {
        return Ok(());
    }
    app::App::default().run(terminal)
}
