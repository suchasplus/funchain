//! mt-rs — Rust port of the Go `mt` markdown viewer.
//! Thin shell: argv parsing and dispatch live in `funchain::mt`.

fn main() -> std::process::ExitCode {
    funchain::mt::run()
}
