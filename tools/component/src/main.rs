use std::{env, fs, io::Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let [input, output]: [std::ffi::OsString; 2] = env::args_os()
        .skip(1)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| "Usage: discord-presence-component-tool <core.wasm> <extension.wasm>")?;
    let core = fs::read(input)?;
    let component = wit_component::ComponentEncoder::default()
        .module(&core)?
        .validate(true)
        .encode()?;
    let mut destination = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&output)?;
    if let Err(error) = destination
        .write_all(&component)
        .and_then(|()| destination.sync_all())
    {
        drop(destination);
        let _ = fs::remove_file(output);
        return Err(error.into());
    }
    Ok(())
}
