//! Print image sizes of a Zl library range: zlinfo <file> <from> <to>.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let lib = mir_formats::zl::ZlLibrary::open(&args[1]).expect("open");
    let (from, to): (usize, usize) = (args[2].parse().unwrap(), args[3].parse().unwrap());
    println!("len {}", lib.len());
    for i in from..=to {
        match lib.info(i) {
            Some(info) => println!(
                "{i}: {}x{} off {},{}",
                info.width, info.height, info.offset_x, info.offset_y
            ),
            None => println!("{i}: none"),
        }
    }
}
