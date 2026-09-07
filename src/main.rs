mod commands;
mod parser;
mod shell;
mod terminal;

#[cfg(test)]
mod testing;

fn main() {
    if let Err(error) = shell::run() {
        eprintln!("0-shell: {error}");
    }
}
