pub fn execute() -> anyhow::Result<()> {
    println!("rustlab {}", env!("CARGO_PKG_VERSION"));
    println!("DSP toolkit — FIR/IIR filters, convolution, windowing");
    println!("Scripting: rustlab run script.rlab");
    println!("Filters:   rustlab filter fir --cutoff 1000 --sr 44100");
    println!("Windows:   rustlab window --type hann --length 64 --plot");
    Ok(())
}
