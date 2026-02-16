use crate::app::App;

mod action;
mod app;
mod config;
mod event;
mod keymap;
mod players;
mod sources;
mod ui;
mod utils;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let result = App::new().run(terminal);
    ratatui::restore();
    result
}
