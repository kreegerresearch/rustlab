/// Real versus complex array storage must not be observable.
///
/// Every example script runs with real storage (the default) and with
/// `RUSTLAB_COMPLEX_STORAGE=1` (every array complex-stored, as before
/// 0.3.17). Standard output and every file the script writes must be
/// byte-identical. A script whose output differs between two runs in the
/// same mode (unseeded random numbers, timings) cannot be compared and is
/// listed as skipped instead.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

/// Standard output plus every file in the script's folder afterwards.
#[derive(PartialEq)]
struct Outcome {
    ok: bool,
    stdout: Vec<u8>,
    files: BTreeMap<String, Vec<u8>>,
}

/// Run a copy of `script`'s folder in a temporary directory. `rustlab run`
/// works in the script's own folder, so outputs land there; the copy keeps
/// them out of the source tree and lets them be compared.
fn run(script: &Path, complex: bool) -> Outcome {
    let dir = tempfile::TempDir::new().unwrap();
    let folder = script.parent().unwrap();
    for f in std::fs::read_dir(folder).unwrap() {
        let p = f.unwrap().path();
        if p.is_file() {
            std::fs::copy(&p, dir.path().join(p.file_name().unwrap())).unwrap();
        }
    }
    let copy = dir.path().join(script.file_name().unwrap());
    let out = Command::new(env!("CARGO_BIN_EXE_rustlab"))
        .args(["run", "--plot", "none", copy.to_str().unwrap()])
        .current_dir(dir.path())
        .env("RUSTLAB_COMPLEX_STORAGE", if complex { "1" } else { "0" })
        .env("RUSTLAB_NO_HISTORY", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let mut files = BTreeMap::new();
    for entry in walk(dir.path()) {
        let rel = entry.strip_prefix(dir.path()).unwrap().to_string_lossy().into_owned();
        files.insert(rel, std::fs::read(&entry).unwrap());
    }
    // The script path appears in the banner; make it independent of the
    // temporary directory's name.
    let stdout = String::from_utf8_lossy(&out.stdout).replace(dir.path().to_str().unwrap(), "<dir>").into_bytes();
    Outcome { ok: out.status.success(), stdout, files }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn first_difference(a: &Outcome, b: &Outcome) -> String {
    if a.ok != b.ok {
        return format!("exit status {} vs {}", a.ok, b.ok);
    }
    if a.stdout != b.stdout {
        let (x, y) = (String::from_utf8_lossy(&a.stdout), String::from_utf8_lossy(&b.stdout));
        for (i, (p, q)) in x.lines().zip(y.lines()).enumerate() {
            if p != q {
                return format!("stdout line {}:\n  real:    {p}\n  complex: {q}", i + 1);
            }
        }
        return "stdout length differs".to_string();
    }
    let names: Vec<_> = a.files.keys().chain(b.files.keys()).collect();
    for n in names {
        if a.files.get(n) != b.files.get(n) {
            return format!("file {n} differs");
        }
    }
    "no difference".to_string()
}

#[test]
fn examples_do_not_depend_on_array_storage() {
    let mut scripts: Vec<PathBuf> = walk(&workspace_root().join("examples"))
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rlab"))
        // Audio scripts stream raw PCM from stdin for as long as it lasts.
        .filter(|p| !p.components().any(|c| c.as_os_str() == "audio"))
        .collect();
    scripts.sort();
    assert!(scripts.len() > 50, "found only {} example scripts", scripts.len());
    // Scripts are independent; run them on all cores.
    enum Verdict {
        Same,
        Skipped(String),
        Differs(String),
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let verdicts = std::sync::Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(script) = scripts.get(i) else { break };
                let name = script.strip_prefix(workspace_root()).unwrap().display().to_string();
                let real = run(script, false);
                let verdict = if real != run(script, false) {
                    Verdict::Skipped(name)
                } else {
                    let complex = run(script, true);
                    if real == complex {
                        Verdict::Same
                    } else {
                        Verdict::Differs(format!("{name}: {}", first_difference(&real, &complex)))
                    }
                };
                verdicts.lock().unwrap().push(verdict);
            });
        }
    });
    let (mut compared, mut skipped, mut failures) = (0, Vec::new(), Vec::new());
    for v in verdicts.into_inner().unwrap() {
        match v {
            Verdict::Same => compared += 1,
            Verdict::Skipped(n) => skipped.push(n),
            Verdict::Differs(d) => {
                compared += 1;
                failures.push(d);
            }
        }
    }
    skipped.sort();
    eprintln!("compared {compared} examples; skipped (not repeatable): {skipped:?}");
    assert!(failures.is_empty(), "storage is observable:\n{}", failures.join("\n"));
    assert!(compared >= 40, "only {compared} examples were repeatable");
}
