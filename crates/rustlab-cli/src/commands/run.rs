use anyhow::{Context, Result};
use clap::{Args, ValueEnum};

/// Where plot commands should render when running a script non-interactively.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum PlotMode {
    /// Default. Each plot opens the TUI pager (skipped automatically when stdout is not a TTY).
    Tui,
    /// Suppress TUI rendering entirely. `savefig()` still writes files — useful for CI and batch runs.
    None,
    /// Auto-connect to rustlab-viewer before running the script (equivalent to `viewer on`).
    /// Falls back to TUI with a warning if the viewer isn't reachable.
    Viewer,
}

#[derive(Args)]
pub struct RunArgs {
    /// Path to a .rlab script file
    pub script: std::path::PathBuf,

    /// Profile all function calls and print a report to stderr on exit.
    /// For selective profiling, add profile(fn1, fn2) inside the script instead.
    #[arg(long)]
    pub profile: bool,

    /// Where plot commands render: `tui` (default), `none`, or `viewer`.
    #[arg(long, value_enum, default_value_t = PlotMode::Tui)]
    pub plot: PlotMode,

    /// When `--plot viewer`, connect to a named rustlab-viewer session.
    #[arg(long, value_name = "NAME")]
    pub viewer_name: Option<String>,
}

pub fn execute(args: RunArgs) -> Result<()> {
    let script = args
        .script
        .canonicalize()
        .with_context(|| format!("failed to resolve path {:?}", args.script))?;
    // Always-on banner: rustlab explicitly identifies itself as the handler
    // for .rlab files. Goes to stderr so script stdout stays clean for
    // pipelines. The user-facing `rlab` extension is rustlab's own — until
    // a native GitHub Linguist definition exists, MATLAB highlighting is
    // used as a temporary visual proxy (see .gitattributes).
    eprintln!(
        "rustlab {} — interpreting {} (.rlab)",
        env!("CARGO_PKG_VERSION"),
        script.display()
    );
    let source =
        std::fs::read_to_string(&script).with_context(|| format!("failed to read {:?}", script))?;
    if let Some(dir) = script.parent() {
        std::env::set_current_dir(dir).with_context(|| format!("failed to chdir to {:?}", dir))?;
    }

    apply_plot_mode(args.plot, args.viewer_name.as_deref());

    // Use run_script_source to support report directives in scripts.
    // Fall back to direct run for profiling mode (no report support needed).
    if args.profile {
        match rustlab_script::run_profiled(&source) {
            Ok(()) => Ok(()),
            Err(rustlab_script::ScriptError::AudioEof) => Ok(()),
            Err(rustlab_script::ScriptError::Interrupted) => Ok(()),
            Err(e) => Err(anyhow::anyhow!("{}", e)),
        }
    } else {
        let mut ev = rustlab_script::Evaluator::new();
        super::repl::run_script_source(&source, &mut ev);
        Ok(())
    }
}

fn apply_plot_mode(mode: PlotMode, viewer_name: Option<&str>) {
    use rustlab_plot::{set_plot_context, PlotContext};
    match mode {
        PlotMode::Tui => { /* default */ }
        PlotMode::None => set_plot_context(PlotContext::Headless),
        PlotMode::Viewer => {
            #[cfg(feature = "viewer")]
            {
                let result = match viewer_name {
                    Some(name) => rustlab_plot::connect_viewer_named(name),
                    None => rustlab_plot::connect_viewer(),
                };
                match result {
                    Ok(true) => {
                        let fig_id = rustlab_plot::viewer_live::get_viewer_fig_id().unwrap_or(1);
                        rustlab_plot::set_current_figure_output(
                            rustlab_plot::FigureOutput::Viewer(fig_id),
                        );
                        match viewer_name {
                            Some(n) => eprintln!("viewer: connected to session '{}' — plots will render in rustlab-viewer", n),
                            None    => eprintln!("viewer: connected — plots will render in rustlab-viewer"),
                        }
                    }
                    Ok(false) => {
                        match viewer_name {
                            Some(n) => eprintln!("viewer: could not connect to session '{}' — is rustlab-viewer --name {} running?", n, n),
                            None    => eprintln!("viewer: could not connect — is rustlab-viewer running?"),
                        }
                        eprintln!("  falling back to TUI rendering");
                    }
                    Err(e) => {
                        eprintln!("viewer: connection failed — {}", e);
                        eprintln!("  falling back to TUI rendering");
                    }
                }
            }
            #[cfg(not(feature = "viewer"))]
            {
                let _ = viewer_name;
                eprintln!("viewer: not available in this build (rebuild with --features viewer)");
                eprintln!("  falling back to TUI rendering");
            }
        }
    }
}
