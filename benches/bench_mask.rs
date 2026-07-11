#![feature(random)]
use std::{
    hint::black_box,
    random::{Rng, SystemRng},
};

use criterion::{Criterion, criterion_group, criterion_main};
use tokio_websockets::mask::frame;

fn mask_benchmark(c: &mut Criterion) {
    // Generate 1 GiB of random data
    let mut data = vec![0; 1024 * 1024 * 1024];
    SystemRng.fill_bytes(&mut data);
    // Generate a random masking key
    let mut key = [0; 4];
    SystemRng.fill_bytes(&mut data);

    // We benchmark 2^n bytes of input for n in 0..=30 (2 ** 30 bytes are 1GiB)
    for exp in 0..=30 {
        let bytes = 2usize.pow(exp);
        c.bench_function(&format!("mask {bytes} bytes"), |b| {
            b.iter(|| frame(black_box(&mut key), black_box(&mut data[..bytes])))
        });
    }
}

criterion_group!(benches, mask_benchmark);
criterion_main!(benches);
