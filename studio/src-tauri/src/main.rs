// Prevents an additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match edits_studio_lib::Startup::parse(&args) {
        Ok(Some(startup)) => edits_studio_lib::run(startup),
        Ok(None) => print!("{}", edits_studio_lib::USAGE),
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!(
                "usage: edits-studio [project.json] [options]   (edits-studio --help lists them)"
            );
            std::process::exit(2);
        }
    }
}
