mod commands;
mod parser;
mod shell;

fn main() {
    if let Err(error) = shell::run() {
        eprintln!("0-shell: {error}");
    }
}
