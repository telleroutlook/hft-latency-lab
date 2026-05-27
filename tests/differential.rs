use std::hint::black_box;

#[test]
fn cross_crate_differential() {
    let (natural, shuffled) = hft_latency_lab::data::gen::generate_paired_streams(1000, 500, 200);

    for (label, data) in &[("natural", &natural), ("shuffled", &shuffled)] {
        let naive = hft_latency_lab::parser::naive::parse_all(black_box(data));
        let opt = hft_latency_lab::parser::optimized::parse_all(black_box(data));

        assert_eq!(naive.len(), opt.len(), "[{label}] message count mismatch");
        for (i, (n, o)) in naive.iter().zip(opt.iter()).enumerate() {
            assert_eq!(n, o, "[{label}] msg {i} mismatch");
        }
    }
}

#[test]
fn cross_crate_full_stream_differential() {
    let stream = hft_latency_lab::data::gen::generate_full_stream(100);
    let naive = hft_latency_lab::parser::naive::parse_all(&stream);
    let opt = hft_latency_lab::parser::optimized::parse_all(&stream);

    assert_eq!(naive.len(), opt.len());
    for (i, (n, o)) in naive.iter().zip(opt.iter()).enumerate() {
        assert_eq!(n, o, "full stream msg {i} mismatch");
    }
}
