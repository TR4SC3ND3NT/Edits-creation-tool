//! Temporary driver: `calib_run <cases> <seed> <out> [degrade] [case-file]`.
use edits_calib::harness;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.get(1).map(|s| s == "case").unwrap_or(false) {
        let r = harness::replay(std::path::Path::new(&a[2]), std::path::Path::new(&a[3]))?;
        print!("{}", harness::describe(&r.outcome));
        print!("{}", edits_calib::report::table(&r.report));
        return Ok(());
    }
    let opts = harness::RunOpts {
        cases: a[1].parse()?,
        seed: a[2].parse()?,
        out: a[3].clone().into(),
        degrade: if a.get(4).map(|s| s == "degrade").unwrap_or(false) { Some(Default::default()) } else { None },
        ..Default::default()
    };
    let r = harness::run(&opts, |l| eprintln!("{l}"))?;
    print!("{}", edits_calib::report::table(&r));
    Ok(())
}
