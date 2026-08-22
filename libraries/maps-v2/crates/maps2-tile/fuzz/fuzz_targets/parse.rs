//! Fuzz TileView::parse — never panics contract `lib.rs:74`.
//! Run: cargo fuzz run parse -- -max_total_time=60
#![no_main]
use libfuzzer_sys::fuzz_target;
use maps2_tile::TileView;

fuzz_target!(|data: &[u8]| {
    let _ = TileView::parse(data);
});
