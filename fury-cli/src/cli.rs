use clap::{Parser, Subcommand, ValueEnum};

/// Supported source languages for parsing.
#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum LangFrom {
    Python,
}

/// Supported target languages for code generation.
#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum LangTo {
    Ts,
}

/// Fury Serialization Ecosystem CLI.
/// Handles code generation and ecosystem management.
#[derive(Parser, Debug)]
#[command(
    name = "fury",
    about = "Fury Serialization Ecosystem CLI",
    propagate_version = true
)]
pub struct FuryCli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Defines the available subcommands for the CLI.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Generate highly optimized cross-language models from registry schemas.
    Generate {
        /// Source language context.
        #[arg(short = 'f', long, value_enum)]
        lang_from: Option<LangFrom>,

        /// Target language for code generation.
        #[arg(short = 't', long, value_enum)]
        lang_to: LangTo,

        /// Source file path, or 'all' to dump everything from the global registry.
        #[arg(short = 's', long, default_value = "all")]
        src: String,

        /// Destination directory path for generated files.
        #[arg(short = 'd', long)]
        dst: String,
    },
}
