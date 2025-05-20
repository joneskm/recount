use clap::Parser;
use std::env::args;
use std::path::PathBuf;
use std::{fs::read_to_string, process::ExitCode};

use recount::{parser::parse, tokenizer::Tokenizer};

const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// The Beancount input filename to load
    #[arg(short, long, value_name = "FILE")]
    file: PathBuf,
}

fn main() -> ExitCode {
    match run() {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            let arg0 = args().next().unwrap_or(CRATE_NAME.to_string());
            eprintln!("{}: {}", arg0, e);
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();

    let buffer = read_to_string(&cli.file)
        .map_err(|e| format!("cannot access '{}': {}", cli.file.display(), e))?;

    let tokenizer = Tokenizer::new(buffer);
    let accounts_doc = parse(tokenizer).map_err(|e| format!("parsing error: {}", e))?;

    for (account, balance) in accounts_doc.balances() {
        println!("{:?}", account);
        println!("{:?}", balance);
    }

    Ok(())
}
