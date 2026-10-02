pub fn execute() -> anyhow::Result<()> {
    println!("rustlab {}", env!("CARGO_PKG_VERSION"));
    println!("Builtins: rustlab docs  and  rustlab docs --json");
    println!("Notebooks, themes, and rendering: rustlab-notebook --help");
    Ok(())
}
