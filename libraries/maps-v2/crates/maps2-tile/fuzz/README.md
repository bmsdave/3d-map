# Fuzz TileView::parse
cargo install cargo-fuzz
cargo fuzz run parse -- -max_total_time=60
