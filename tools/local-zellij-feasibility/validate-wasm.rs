//! Original MIT packaging fixture; compile each rebuilt module with the pinned VM.
use std::{error::Error, fs};
use wasmtime::{Config, Engine, Module, Strategy};

fn main() -> Result<(), Box<dyn Error>> {
    let paths: Vec<_> = std::env::args_os().skip(1).collect();
    if paths.len() != 10 {
        return Err("expected exactly ten release plugin modules".into());
    }
    // The pinned server's get_engine uses this same public Cranelift strategy.
    let engine = Engine::new(Config::new().strategy(Strategy::Cranelift))?;
    for path in paths {
        let size = fs::metadata(&path)?.len();
        if !(8..=16 * 1024 * 1024).contains(&size) {
            return Err("module exceeds the producer byte bound".into());
        }
        let bytes = fs::read(&path)?;
        let module = Module::new(&engine, &bytes)?;
        println!(
            "VALIDATED {} {}",
            std::path::Path::new(&path).display(),
            bytes.len()
        );
        drop(module);
    }
    Ok(())
}
