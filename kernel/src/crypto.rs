//! Cryptographic primitives used by the kernel and boot paths.
//!
//! This module starts with the two hashes the platform actually needs in the
//! kernel — SHA-256 and HMAC-SHA256 — plus a constant-time comparison helper.
//! They are implemented here rather than pulled from a crate so that the kernel
//! stays dependency-free and so that a hardware-backed implementation can be
//! dropped in behind the same API in Phase 3 (`subsys/crypto`).
//!
//! Signature verification (Ed25519), AEAD (AES-GCM) and key storage are **not**
//! here: they are large, easy to get subtly wrong, and arrive with the RustCrypto
//! backends in Phase 3/5. What is here is pinned by NIST/RFC vectors.

/// Output size of SHA-256 in bytes.
pub const SHA256_OUTPUT: usize = 32;

/// Block size of SHA-256 in bytes.
pub const SHA256_BLOCK: usize = 64;

const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// Streaming SHA-256.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; SHA256_BLOCK],
    buffered: usize,
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    /// Start a new hash.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: [
                0x6a09_e667,
                0xbb67_ae85,
                0x3c6e_f372,
                0xa54f_f53a,
                0x510e_527f,
                0x9b05_688c,
                0x1f83_d9ab,
                0x5be0_cd19,
            ],
            buffer: [0; SHA256_BLOCK],
            buffered: 0,
            length: 0,
        }
    }

    /// Hash `data` in one call.
    #[must_use]
    pub fn digest(data: &[u8]) -> [u8; SHA256_OUTPUT] {
        let mut hasher = Self::new();
        hasher.update(data);
        hasher.finalize()
    }

    /// Feed more input.
    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);

        if self.buffered > 0 {
            let take = (SHA256_BLOCK - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == SHA256_BLOCK {
                self.compress();
                self.buffered = 0;
            }
        }

        while data.len() >= SHA256_BLOCK {
            self.buffer.copy_from_slice(&data[..SHA256_BLOCK]);
            self.compress();
            data = &data[SHA256_BLOCK..];
        }

        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffered = data.len();
        }
    }

    /// Finish the hash and return the 32-byte digest.
    #[must_use]
    pub fn finalize(mut self) -> [u8; SHA256_OUTPUT] {
        let bit_length = self.length.wrapping_mul(8);

        self.buffer[self.buffered] = 0x80;
        self.buffered += 1;
        if self.buffered > 56 {
            self.buffer[self.buffered..].fill(0);
            self.compress();
            self.buffered = 0;
        }
        self.buffer[self.buffered..56].fill(0);
        self.buffer[56..64].copy_from_slice(&bit_length.to_be_bytes());
        self.compress();

        let mut output = [0u8; SHA256_OUTPUT];
        for (index, word) in self.state.iter().enumerate() {
            output[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        output
    }

    fn compress(&mut self) {
        let mut w = [0u32; 64];
        for (index, chunk) in self.buffer.as_chunks::<4>().0.iter().enumerate() {
            w[index] = u32::from_be_bytes(*chunk);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        for (state, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *state = state.wrapping_add(value);
        }
    }
}

/// Streaming HMAC-SHA256 (RFC 2104).
#[derive(Clone)]
pub struct HmacSha256 {
    inner: Sha256,
    outer_pad: [u8; SHA256_BLOCK],
}

impl HmacSha256 {
    /// Start an HMAC with `key`. Keys longer than the block size are hashed first.
    #[must_use]
    pub fn new(key: &[u8]) -> Self {
        let mut block = [0u8; SHA256_BLOCK];
        if key.len() > SHA256_BLOCK {
            block[..SHA256_OUTPUT].copy_from_slice(&Sha256::digest(key));
        } else {
            block[..key.len()].copy_from_slice(key);
        }

        let mut inner_pad = [0x36u8; SHA256_BLOCK];
        let mut outer_pad = [0x5cu8; SHA256_BLOCK];
        for index in 0..SHA256_BLOCK {
            inner_pad[index] ^= block[index];
            outer_pad[index] ^= block[index];
        }

        let mut inner = Sha256::new();
        inner.update(&inner_pad);
        Self { inner, outer_pad }
    }

    /// Feed more message data.
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    /// Finish and return the 32-byte MAC.
    #[must_use]
    pub fn finalize(self) -> [u8; SHA256_OUTPUT] {
        let inner_hash = self.inner.finalize();
        let mut outer = Sha256::new();
        outer.update(&self.outer_pad);
        outer.update(&inner_hash);
        outer.finalize()
    }

    /// One-shot HMAC.
    #[must_use]
    pub fn mac(key: &[u8], data: &[u8]) -> [u8; SHA256_OUTPUT] {
        let mut hmac = Self::new(key);
        hmac.update(data);
        hmac.finalize()
    }
}

/// Compare two byte strings without an early exit.
///
/// Length is still observable; the contents are not. Use it for MACs, tags and
/// digests, never for user input.
#[must_use]
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> std::string::String {
        use std::fmt::Write;
        let mut output = std::string::String::new();
        for byte in bytes {
            write!(output, "{byte:02x}").unwrap();
        }
        output
    }

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            hex(&Sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&Sha256::digest(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_incremental_matches_one_shot() {
        let parts: [&[u8]; 3] = [b"a", b"bc", b"defghijklmnopqrstuvwxyz"];
        let mut hasher = Sha256::new();
        for part in parts {
            hasher.update(part);
        }
        assert_eq!(
            hasher.finalize(),
            Sha256::digest(b"abcdefghijklmnopqrstuvwxyz")
        );
    }

    #[test]
    fn sha256_handles_padding_boundaries() {
        // 55/56/64 bytes exercise the "fits" / "needs another block" path.
        for length in [55usize, 56, 63, 64, 65, 119, 128] {
            let data = std::vec![0xA5u8; length];
            assert_eq!(Sha256::digest(&data), {
                let mut hasher = Sha256::new();
                hasher.update(&data);
                hasher.finalize()
            });
        }
    }

    #[test]
    fn sha256_million_a() {
        let mut hasher = Sha256::new();
        let block = [b'a'; 1000];
        for _ in 0..1000 {
            hasher.update(&block);
        }
        assert_eq!(
            hex(&hasher.finalize()),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn hmac_rfc4231_vectors() {
        assert_eq!(
            hex(&HmacSha256::mac(&[0x0b; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            hex(&HmacSha256::mac(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            hex(&HmacSha256::mac(&[0xaa; 20], &[0xdd; 50])),
            "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe"
        );
    }

    #[test]
    fn hmac_hashes_long_keys() {
        let key = [0xaa; 131];
        assert_eq!(
            hex(&HmacSha256::mac(
                &key,
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn hmac_supports_incremental_input() {
        let mut hmac = HmacSha256::new(b"Jefe");
        hmac.update(b"what do ya want ");
        hmac.update(b"for nothing?");
        assert_eq!(
            hmac.finalize(),
            HmacSha256::mac(b"Jefe", b"what do ya want for nothing?")
        );
    }

    #[test]
    fn constant_time_eq_compares_contents() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"diff"));
        assert!(!constant_time_eq(b"short", b"longer"));
        assert!(constant_time_eq(&[], &[]));
    }
}
