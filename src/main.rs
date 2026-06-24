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
        Ok(rmus::cli::CliAction::Status) => {
            print!("{}", rmus::cli::status().message());
            return Ok(());
        }
        Ok(rmus::cli::CliAction::Doctor) => {
            let report = rmus::cli::doctor_report();
            print!("{}", report.to_text());
            std::process::exit(report.exit_code());
        }
        Ok(rmus::cli::CliAction::Paths) => {
            print!("{}", rmus::cli::paths_text());
            return Ok(());
        }
        Ok(rmus::cli::CliAction::Completions { shell }) => {
            print!("{}", rmus::cli::completions_text(shell));
            return Ok(());
        }
        Ok(rmus::cli::CliAction::ListSources) => {
            print!("{}", rmus::cli::list_sources().message());
            return Ok(());
        }
        Ok(rmus::cli::CliAction::ListPlaylists) => {
            print!("{}", rmus::cli::list_playlists().message());
            return Ok(());
        }
        Ok(rmus::cli::CliAction::ShowHistory { limit }) => {
            print!("{}", rmus::cli::show_history_with_limit(limit).message());
            return Ok(());
        }
        Ok(rmus::cli::CliAction::ShowQueue { limit }) => {
            print!("{}", rmus::cli::show_queue_with_limit(limit).message());
            return Ok(());
        }
        Ok(rmus::cli::CliAction::LocalStats) => {
            let summary = rmus::cli::local_stats();
            println!("{}", summary.message());
            return Ok(());
        }
        Ok(rmus::cli::CliAction::SearchLocal { query, limit }) => {
            match rmus::cli::search_local(&query, limit) {
                Ok(summary) => {
                    print!("{}", summary.message());
                    return Ok(());
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            }
        }
        Ok(rmus::cli::CliAction::ScanLocal { name }) => {
            match rmus::cli::scan_local(name.as_deref()) {
                Ok(summary) => {
                    println!("{}", summary.message());
                    return Ok(());
                }
                Err(rmus::cli::LocalScanError::Io(error)) => return Err(error.into()),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            }
        }
        Ok(rmus::cli::CliAction::AddSource { name, path, scan }) => {
            let result = if scan {
                rmus::cli::add_source_and_scan(&name, &path).map(|summary| summary.message())
            } else {
                rmus::cli::add_source(&name, &path).map(|summary| summary.message())
            };
            match result {
                Ok(summary) => {
                    println!("{summary}");
                    return Ok(());
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            }
        }
        Ok(rmus::cli::CliAction::RemoveSource { name }) => match rmus::cli::remove_source(&name) {
            Ok(summary) => {
                println!("{}", summary.message());
                return Ok(());
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        },
        Ok(rmus::cli::CliAction::MoveSource { name, target }) => {
            match rmus::cli::move_source(&name, target) {
                Ok(summary) => {
                    println!("{}", summary.message());
                    return Ok(());
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            }
        }
        Ok(rmus::cli::CliAction::ShowPlaylist { name, limit }) => {
            match rmus::cli::show_playlist_with_limit(&name, limit) {
                Ok(summary) => {
                    print!("{}", summary.message());
                    return Ok(());
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            }
        }
        Ok(rmus::cli::CliAction::CreatePlaylist { name }) => {
            match rmus::cli::create_playlist(&name) {
                Ok(summary) => {
                    println!("{}", summary.message());
                    return Ok(());
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            }
        }
        Ok(rmus::cli::CliAction::RenamePlaylist { name, new_name }) => {
            match rmus::cli::rename_playlist(&name, &new_name) {
                Ok(summary) => {
                    println!("{}", summary.message());
                    return Ok(());
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            }
        }
        Ok(rmus::cli::CliAction::DuplicatePlaylist { name, new_name }) => {
            match rmus::cli::duplicate_playlist(&name, new_name.as_deref()) {
                Ok(summary) => {
                    println!("{}", summary.message());
                    return Ok(());
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            }
        }
        Ok(rmus::cli::CliAction::DeletePlaylist { name }) => {
            match rmus::cli::delete_playlist(&name) {
                Ok(summary) => {
                    println!("{}", summary.message());
                    return Ok(());
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            }
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
        Ok(rmus::cli::CliAction::ClearAccounts) => match rmus::cli::clear_accounts() {
            Ok(summary) => {
                println!("{}", summary.message());
                return Ok(());
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        },
        Ok(rmus::cli::CliAction::ClearHistory) => {
            let summary = rmus::cli::clear_history()?;
            println!("{}", summary.message());
            return Ok(());
        }
        Ok(rmus::cli::CliAction::ClearQueue) => {
            let summary = rmus::cli::clear_queue()?;
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
    let mouse_capture_result =
        crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
    let result = match mouse_capture_result {
        Ok(()) => rmus::app::App::new().run(terminal),
        Err(error) => Err(error.into()),
    };
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
    ratatui::restore();
    result
}
