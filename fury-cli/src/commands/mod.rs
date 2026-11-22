pub mod generate;

use crate::cli::{Commands, FuryCli};
use crate::errors::CliError;

/// Routes the parsed CLI application data to the appropriate command handler.
pub fn execute_command(cli_app: FuryCli) -> Result<(), CliError> {
    match cli_app.command {
        Commands::Generate {
            lang_from,
            lang_to,
            src,
            dst,
        } => generate::handle_generate(lang_from, lang_to, src, dst),
    }
}
