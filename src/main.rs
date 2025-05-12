use clap::Parser;
use std::fs::read_to_string;
use std::path::PathBuf;

use recount::{parser::parse, tokenizer::Tokenizer};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// The Beancount input filename to load
    #[arg(short, long, value_name = "FILE")]
    file: PathBuf,
}

fn main() {
    let cli = Cli::parse();

    let buffer = read_to_string(cli.file).unwrap();
    let tokenizer = Tokenizer::new(buffer);
    let accounts_doc = parse(tokenizer).unwrap();

    for (account, balance) in accounts_doc.balances() {
        println!("{:?}", account);
        println!("{:?}", balance);
    }
}
