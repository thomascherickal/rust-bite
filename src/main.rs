#![cfg_attr(
    all(not(debug_assertions), target_family = "windows"),
    windows_subsystem = "windows"
)]

#[cfg(not(any(target_family = "windows", target_family = "unix")))]
compile_error!("Bite can only be build for windows, macos and linux.");

mod wayland;

use args::ARGS;
use std::fs;

fn main() {
    #[cfg(target_os = "linux")]
    if unsafe { libc::getuid() } == 0 {
        wayland::set_env();
    }

    if ARGS.disassemble {
        #[cfg(target_family = "windows")]
        let mut ui = gui::UI::<gui::windows::Arch>::new().unwrap();

        #[cfg(target_family = "unix")]
        let mut ui = gui::UI::<gui::unix::Arch>::new().unwrap();

        ui.process_args();
        ui.run();

        return;
    }

    let binary = fs::read(ARGS.path.as_ref().unwrap()).expect("Unexpected read of binary failed.");
    let obj = object::File::parse(&*binary).expect("Not a valid object.");
    let path = ARGS.path.as_ref().unwrap().display();

    if ARGS.libs {
        let mut index = symbols::Index::new();

        if let Err(err) = index.parse_imports(&binary, &obj) {
            eprintln!("Failed to parse import table ({err:?})");
            std::process::exit(0);
        }

        if index.is_empty() {
            eprintln!("Object \"{path}\" doesn't seem to import anything.");
            std::process::exit(0);
        }

        println!("{path}:");

        for function in index.symbols() {
            let symbol = function.name().iter().map(|s| &s.text[..]);
            let symbol = String::from_iter(symbol);

            match function.module() {
                Some(module) => println!("\t{} => {symbol}", &*module.text),
                None => println!("\t{symbol}"),
            };
        }
    }

    if ARGS.names {
        let mut index = symbols::Index::new();

        if let Err(err) = index.parse_debug(&obj) {
            eprintln!("Failed to parse symbol table ({err:?})");
            std::process::exit(1);
        }

        if index.is_empty() {
            eprintln!("Object \"{path}\" doesn't seem to export any symbols.");
            std::process::exit(0);
        }

        for function in index.symbols() {
            let symbol = function.name().iter().map(|s| &s.text[..]);
            let symbol = String::from_iter(symbol);

            println!("{symbol}");
        }
    }
}
