use blake3;

pub fn blake(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

