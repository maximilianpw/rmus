fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let result = rmus::app::App::new().run(terminal);
    ratatui::restore();
    result
}
