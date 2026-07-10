//! Optical search — deterministic perceptual-hash image matching.
//!
//! Replaces the TS-retired `optical search` behavior as real, tested Rust.
//! Operates on a grayscale image as a flat `&[u8]` of `width*height` luminance
//! samples in [0,255]. It computes an 8x8 average hash (aHash): downscale to
//! 8x8, threshold each pixel against the mean, pack bits. Similar images hash
//! close in Hamming distance. NO rng, NO clock, NO external image libs — the
//! pixel buffer is the input contract (a real loader would feed decoded pixels).
//!
//! This is the deterministic core; a real decoder (png/jpeg) feeds it pixels.

/// A grayscale image view.
pub struct Gray<'a> {
    pub width: usize,
    pub height: usize,
    pub pixels: &'a [u8], // length == width*height, luminance 0..=255
}

impl<'a> Gray<'a> {
    pub fn new(width: usize, height: usize, pixels: &'a [u8]) -> Self {
        Gray { width, height, pixels }
    }

    fn at(&self, x: usize, y: usize) -> u8 {
        self.pixels[y * self.width + x]
    }
}

/// Compute an 8x8 average hash (64 bits) for an image.
/// Steps: box-downsample to 8x8 by averaging 8x8-ish blocks, then threshold each
/// against the 8x8 mean.
pub fn ahash(img: &Gray) -> u64 {
    const N: usize = 8;
    let mut small = [0u32; N * N];
    // each block covers (width/N) x (height/N) source pixels
    let bw = (img.width + N - 1) / N;
    let bh = (img.height + N - 1) / N;
    for sy in 0..N {
        for sx in 0..N {
            let mut sum: u32 = 0;
            let mut cnt: u32 = 0;
            for y in (sy * bh)..(((sy + 1) * bh).min(img.height)) {
                for x in (sx * bw)..(((sx + 1) * bw).min(img.width)) {
                    sum += img.at(x, y) as u32;
                    cnt += 1;
                }
            }
            small[sy * N + sx] = if cnt > 0 { sum / cnt } else { 0 };
        }
    }
    // mean of the 8x8
    let mean = small.iter().sum::<u32>() / (N * N) as u32;
    let mut bits: u64 = 0;
    for i in 0..(N * N) {
        if small[i] as u32 >= mean {
            bits |= 1u64 << i;
        }
    }
    bits
}

/// Hamming distance between two 64-bit hashes (number of differing bits).
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// A stored reference image with its hash.
#[derive(Clone)]
pub struct Ref {
    pub id: String,
    pub hash: u64,
}

/// The optical index: a set of reference hashes to search against.
#[derive(Default)]
pub struct OpticalIndex {
    refs: Vec<Ref>,
}

impl OpticalIndex {
    pub fn new() -> Self {
        OpticalIndex::default()
    }

    pub fn add(&mut self, id: &str, img: &Gray) {
        self.refs.push(Ref { id: id.to_string(), hash: ahash(img) });
    }

    /// Find the closest reference within `max_dist` Hamming bits. Returns
    /// (id, distance) of the best match, or None if nothing is within threshold.
    pub fn search(&self, img: &Gray, max_dist: u32) -> Option<(String, u32)> {
        let h = ahash(img);
        let mut best: Option<(String, u32)> = None;
        for r in &self.refs {
            let d = hamming(h, r.hash);
            match best {
                None => best = Some((r.id.clone(), d)),
                Some((_, bd)) if d < bd => best = Some((r.id.clone(), d)),
                _ => {}
            }
        }
        match best {
            Some((id, d)) if d <= max_dist => Some((id, d)),
            _ => None,
        }
    }

    pub fn len(&self) -> usize {
        self.refs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(v: u8) -> Vec<u8> {
        vec![v; 16 * 16]
    }

    // top half `a`, bottom half `b` — a non-constant image (aHash is degenerate
    // for constant images, so RED tests use this instead).
    fn stripe(a: u8, b: u8) -> Vec<u8> {
        let mut v = Vec::with_capacity(16 * 16);
        for y in 0..16 {
            for _x in 0..16 {
                v.push(if y < 8 { a } else { b });
            }
        }
        v
    }

    #[test]
    fn identical_images_match_zero_dist() {
        // GREEN: the same pixels hash to 0 Hamming distance.
        let buf = solid(100);
        let a = Gray::new(16, 16, &buf);
        let b = Gray::new(16, 16, &buf);
        assert_eq!(hamming(ahash(&a), ahash(&b)), 0);
    }

    #[test]
    fn similar_images_within_threshold() {
        // GREEN: a mostly-similar image matches within a generous threshold.
        let base_buf = stripe(120, 120);
        let base = Gray::new(16, 16, &base_buf);
        let mut near = stripe(120, 120);
        // poke a few pixels darker
        near[0] = 60;
        near[100] = 60;
        near[200] = 200;
        let near_img = Gray::new(16, 16, &near);
        let mut idx = OpticalIndex::new();
        idx.add("ship_a", &base);
        let m = idx.search(&near_img, 10).expect("should match within 10 bits");
        assert_eq!(m.0, "ship_a");
    }

    #[test]
    fn different_images_excluded_by_threshold() {
        // RED: two genuinely different (non-constant) images must NOT match
        // within a tight threshold. Inverted stripes produce opposing hashes.
        let dark_top = stripe(10, 245);
        let bright_top = stripe(245, 10);
        let base = Gray::new(16, 16, &dark_top);
        let other = Gray::new(16, 16, &bright_top);
        let mut idx = OpticalIndex::new();
        idx.add("dark", &base);
        assert!(
            idx.search(&other, 5).is_none(),
            "dissimilar image wrongly matched"
        );
    }
}
