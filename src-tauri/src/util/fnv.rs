//! FNV-1a 32-bit — хэш `fileFingerprint` в CurseForge API (спека §6.7).

/// FNV-1a 32-bit.
pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &b in data {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(fnv1a32(b""), 0x811c_9dc5);
        assert_eq!(fnv1a32(b"a"), 0xe40c_292c);
        // Вектор из RFC-набора FNV: "foobar"
        assert_eq!(fnv1a32(b"foobar"), 0xbf9c_f968);
    }
}
