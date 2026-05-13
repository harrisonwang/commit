use std::path::Path;

fn main() {
    if let Err(error) = commit_mcp::serve_stdio(Path::new(".")) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
