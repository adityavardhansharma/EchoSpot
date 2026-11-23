//! Captures a region of the first output a few times and writes the last as a PPM.
//! `cargo run -p rune-glass --example grab -- x y w h out.ppm`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let n = |i: usize| a[i].parse::<i64>().unwrap();
    let region = rune_glass::Region { x: n(0) as i32, y: n(1) as i32, w: n(2) as u32, h: n(3) as u32 };
    let cap = rune_glass::OutputCapture::start();
    cap.set(None, Some(region));
    let started = std::time::Instant::now();
    let mut seen = 0;
    while started.elapsed() < std::time::Duration::from_secs(3) {
        if let Some(c) = cap.latest() && c.seq != seen {
            seen = c.seq;
        }
        if let Some(e) = cap.error() { eprintln!("error: {e}"); return; }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let c = cap.latest().expect("a capture");
    println!("{} captures in 3 s; output {}x{}; presented {:.3} s ago", c.seq, c.output.0, c.output.1, (rune_glass::now_ns().saturating_sub(c.presented_ns)) as f64 / 1e9);
    let mut ppm = format!("P6\n{} {}\n255\n", region.w, region.h).into_bytes();
    for px in c.rgba.chunks(4) { ppm.extend(&px[..3]); }
    std::fs::write(&a[4], ppm).unwrap();
}
