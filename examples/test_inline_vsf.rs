// Quick test to create a VSF file with inline fields
use std::io::Write;

fn main() {
    use vsf::{VsfBuilder, VsfType};

    // Create a PT ACK style packet with inline values
    let chunk_hash = [0xAB; 32];
    let bytes = VsfBuilder::new()
        .provenance_hash(chunk_hash)
        .provenance_only()
        .add_inline_field(
            "pt_ack",
            vec![
                VsfType::u3(42), // seq
                VsfType::u3(75), // buf
            ],
        )
        .build()
        .unwrap();

    // Write to file
    let mut file = std::fs::File::create("/tmp/test_pt_ack.vsf").unwrap();
    file.write_all(&bytes).unwrap();

    println!("Wrote {} bytes to /tmp/test_pt_ack.vsf", bytes.len());
    println!("Hex: {:02X?}", &bytes[..std::cmp::min(100, bytes.len())]);
}
