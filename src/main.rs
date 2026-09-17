mod terminal;
fn main() -> std::process::ExitCode {
    match terminal::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Launchpad: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
