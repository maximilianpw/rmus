fn main() -> color_eyre::Result<()> {
    match rmus::cli::parse_args(std::env::args()) {
        Ok(rmus::cli::CliAction::Run) => {}
        Ok(rmus::cli::CliAction::Help) => {
            print!("{}", rmus::cli::help_text());
            return Ok(());
        }
        Ok(rmus::cli::CliAction::Version) => {
            println!("{}", rmus::cli::version_text());
            return Ok(());
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }

    color_eyre::install()?;
    let terminal = ratatui::init();
    let result = rmus::app::App::new().run(terminal);
    ratatui::restore();
    result
}
