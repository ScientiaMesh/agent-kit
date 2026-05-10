use clap::error::ErrorKind;
use clap::Parser;
use smesh_rs::{agent_mode_enabled, output_mode_from_raw_args, run, Cli, CliError, OutputMode};

fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => handle_parse_error(error),
    };
    let output_mode = cli.effective_output();

    match run(cli) {
        Ok(output) => print!("{output}"),
        Err(error) => {
            match output_mode {
                OutputMode::Human => eprintln!("{error}"),
                OutputMode::Json | OutputMode::Ndjson => match error.render(output_mode) {
                    Ok(output) => print!("{output}"),
                    Err(render_error) => eprintln!("{render_error}"),
                },
            }
            std::process::exit(error.exit_code());
        }
    }
}

fn handle_parse_error(error: clap::Error) -> ! {
    let output_mode = output_mode_from_raw_args(std::env::args_os(), agent_mode_enabled());

    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) || output_mode == OutputMode::Human
    {
        error.exit();
    }

    let cli_error = CliError::config(strip_ansi_codes(error.to_string().trim()));
    match cli_error.render(output_mode) {
        Ok(output) => print!("{output}"),
        Err(render_error) => eprintln!("{render_error}"),
    }
    std::process::exit(error.exit_code());
}

fn strip_ansi_codes(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if code.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            stripped.push(ch);
        }
    }

    stripped
}
