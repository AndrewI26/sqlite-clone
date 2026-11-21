mod pager;
mod parser;
mod record;

use std::io::{self, BufRead, Write};

fn main() {
    let mut lines = io::stdin().lock().lines();
    loop {
        print!("db > ");
        io::stdout().flush().unwrap();

        let Some(Ok(line)) = lines.next() else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == ".exit" {
            break;
        }

        match parser::parse(line) {
            Ok(statement) => println!("{statement:#?}"),
            Err(e) => println!("Error: {e}"),
        }
    }
}
