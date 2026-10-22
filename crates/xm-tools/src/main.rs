use std::process::ExitCode;

const BASE_HEIGHT: f32 = 96.0;

fn run(args: &[String]) -> Result<(), String> {
    let usage = "usage: cutout <src.jpg> <dst-stem> --heights 96,144,192";
    let (src, stem) = match args {
        [src, stem, flag, _] if flag == "--heights" => (src, stem),
        _ => return Err(usage.into()),
    };
    let heights: Vec<u32> = args[3]
        .split(',')
        .map(|h| h.trim().parse().map_err(|_| format!("bad height '{h}'")))
        .collect::<Result<_, _>>()?;
    if heights.contains(&0) {
        return Err("heights must be greater than zero".into());
    }
    let rgb = image::open(src)
        .map_err(|e| format!("{src}: {e}"))?
        .to_rgb8();
    let cut = xm_tools::cut_out(&rgb);
    for h in heights {
        let dst = format!("{stem}@{}x.png", h as f32 / BASE_HEIGHT);
        xm_tools::resize_to_height(&cut, h)
            .save(&dst)
            .map_err(|e| format!("{dst}: {e}"))?;
        println!("{dst}");
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
