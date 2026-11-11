//! Pixel statistics of Zl images: zlstats <file> <from> <to>.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let lib = mir_formats::zl::ZlLibrary::open(&args[1]).expect("open");
    let (from, to): (usize, usize) = (args[2].parse().unwrap(), args[3].parse().unwrap());
    for i in from..=to {
        match lib.decode(i, mir_formats::zl::SurfaceKind::Image) {
            Ok(Some(s)) => {
                let px = s.rgba.chunks(4);
                let n = px.len().max(1);
                let (mut a, mut lum, mut opaque) = (0u64, 0u64, 0u64);
                for p in s.rgba.chunks(4) {
                    a += p[3] as u64;
                    if p[3] > 0 {
                        opaque += 1;
                        lum += (p[0] as u64 + p[1] as u64 + p[2] as u64) / 3;
                    }
                }
                println!(
                    "{i}: {}x{} opaque {} / {} mean alpha {} mean lum(opaque) {}",
                    s.width,
                    s.height,
                    opaque,
                    n,
                    a / n as u64,
                    lum / opaque.max(1)
                );
            }
            Ok(None) => println!("{i}: none"),
            Err(e) => println!("{i}: error {e}"),
        }
    }
}
