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
        Ok(rmus::cli::CliAction::Doctor) => {
            let report = rmus::cli::doctor_report();
            print!("{}", report.to_text());
            std::process::exit(report.exit_code());
        }
        Ok(rmus::cli::CliAction::ImportPlaylist { path, name }) => {
            let summary = rmus::cli::import_playlist(&path, name.as_deref())?;
            println!("{}", summary.message());
            return Ok(());
        }
        Ok(rmus::cli::CliAction::ExportPlaylist { name, path }) => {
            let summary = rmus::cli::export_playlist(&name, &path)?;
            println!("{}", summary.message());
            return Ok(());
        }
        Ok(rmus::cli::CliAction::ClearCache) => {
            let summary = rmus::cli::clear_cache()?;
            println!("{}", summary.message());
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
