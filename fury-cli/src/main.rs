mod cli;
mod commands;
mod errors;
mod extractors;
mod generators;

use clap::Parser;
use cli::FuryCli;

pub use fury_core::schema::registry::get_global_registry;


fn main() {
    let args = FuryCli::parse();

    if let Err(e) = commands::execute_command(args) {
        eprintln!("  Fury CLI Error: {}", e);
        std::process::exit(1);
    }
}
