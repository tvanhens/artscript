pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Seed for a node: FNV-1a over `file \0 node_path`.
pub fn node_seed(file: &str, node_path: &str) -> u64 {
    let mut bytes = Vec::with_capacity(file.len() + node_path.len() + 1);
    bytes.extend_from_slice(file.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(node_path.as_bytes());
    fnv1a(&bytes)
}

/// The seed an author's `seed:` sets. It is the same in any node and file.
pub fn explicit_seed(seed: u64) -> u64 {
    Rng::new(seed).next_u64()
}

/// SplitMix64.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}
