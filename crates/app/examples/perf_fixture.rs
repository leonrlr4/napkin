//! Writes [`app::fixture::generate`]'s deterministic 1000-element scene to a `.excalidraw` file,
//! for the `--bench` performance acceptance (spec §6.7, decision 11):
//!
//! ```sh
//! cargo run -p app --example perf_fixture -- OUT.excalidraw [SEED]
//! ```
//!
//! `SEED` defaults to 1; the element count is fixed at 1000, matching the acceptance scene size.

const DEFAULT_SEED: u32 = 1;
const ELEMENT_COUNT: usize = 1000;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(out_path) = args.next() else {
        eprintln!("usage: perf_fixture OUT.excalidraw [SEED]");
        std::process::exit(2);
    };
    let seed = match args.next() {
        Some(arg) => arg.parse::<u32>().unwrap_or_else(|error| {
            eprintln!("perf_fixture: invalid seed {arg:?}: {error}");
            std::process::exit(2);
        }),
        None => DEFAULT_SEED,
    };

    let file = app::fixture::generate(seed, ELEMENT_COUNT);
    std::fs::write(&out_path, file.to_json_string()).unwrap_or_else(|error| {
        eprintln!("perf_fixture: {out_path}: {error}");
        std::process::exit(1);
    });
}
