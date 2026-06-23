#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAction {
    Run,
    Help,
    Version,
}

pub fn parse_args<I, S>(args: I) -> Result<CliAction, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter();
    let _program = args.next();

    let Some(first) = args.next().map(Into::into) else {
        return Ok(CliAction::Run);
    };

    if args.next().is_some() {
        return Err(format!(
            "unexpected argument after '{first}'\n\n{}",
            help_text()
        ));
    }

    match first.as_str() {
        "-h" | "--help" => Ok(CliAction::Help),
        "-V" | "--version" => Ok(CliAction::Version),
        _ => Err(format!("unknown argument '{first}'\n\n{}", help_text())),
    }
}

pub fn version_text() -> String {
    format!("rmus {}", env!("CARGO_PKG_VERSION"))
}

pub fn help_text() -> &'static str {
    concat!(
        "rmus - keyboard-driven terminal music player\n",
        "\n",
        "Usage:\n",
        "  rmus [OPTIONS]\n",
        "\n",
        "Options:\n",
        "  -h, --help       Print help\n",
        "  -V, --version    Print version\n"
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_args, CliAction};

    #[test]
    fn no_args_runs_tui() {
        assert_eq!(parse_args(["rmus"]), Ok(CliAction::Run));
    }

    #[test]
    fn help_flags_print_help() {
        assert_eq!(parse_args(["rmus", "--help"]), Ok(CliAction::Help));
        assert_eq!(parse_args(["rmus", "-h"]), Ok(CliAction::Help));
    }

    #[test]
    fn version_flags_print_version() {
        assert_eq!(parse_args(["rmus", "--version"]), Ok(CliAction::Version));
        assert_eq!(parse_args(["rmus", "-V"]), Ok(CliAction::Version));
    }

    #[test]
    fn unknown_args_are_errors() {
        let error = parse_args(["rmus", "--wat"]).expect_err("unknown flag should fail");

        assert!(error.contains("unknown argument '--wat'"));
        assert!(error.contains("Usage:"));
    }
}
