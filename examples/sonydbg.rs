use exiftool_rs::tags::sony_ciphered_generated as g;
fn main() {
    let data = [0u8; 64];
    for tag in [0x2010u16, 0x9050, 0x9400, 0x9401, 0x9402, 0x9403, 0x9404, 0x9405, 0x9406, 0x940c, 0x940e, 0x3000, 0x202a] {
        let v = g::variant_for(tag, "ILCE-9", &data, 64, "undef", false, false);
        println!("{tag:#06x} -> {:?}", v.map(|x| x.table));
    }
}
