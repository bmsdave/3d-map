import io

p = "crates/maps2-ingest/src/bin/maps2-ingest.rs"
s = io.open(p, encoding="utf-8").read()

# --- the threshold, and why a manifest stops listing its tiles
anchor = "fn manifest_json("
doc = '''/// Above this many tiles a manifest stops naming them one by one.
///
/// A carve is a few hundred tiles and the list is worth having: the
/// client knows before it asks whether a tile exists, and a digest per
/// tile catches a corrupt commit. A planet to z14 is on the order of
/// 10^8, where the same list is gigabytes of JSON that every visitor
/// would download before the first frame — and the digests alone would
/// be six gigabytes of hex.
///
/// So above the threshold the manifest carries the envelope instead:
/// which levels exist and, per level, the ground they cover. The client
/// computes tile URLs and treats a 404 as "no tile there", which is what
/// every tile server on the web already does. `verify-package` still
/// walks the whole directory and checks every byte — that check moves to
/// the build, where the bytes are, rather than to each viewer.
const MAX_ENUMERATED_TILES: usize = 50_000;

'''
assert s.count(anchor) == 2  # definition and one test call
s = s.replace(anchor, doc + anchor, 1)

old = '''    serde_json::to_string_pretty(&json!({
        "format": "MT2",
        "format_version": maps2_tile::FORMAT_VERSION,
        "levels": levels,
        "feature_count": feature_count,
        "tile_count": tile_digests.len(),
        "tiles": tile_paths(tile_digests),
        "tile_digests": digest_map(tile_digests),
        "package_sha256": package_sha256(tile_digests),
        "view": package_view(tile_digests),
        "height_tile_count": height_tile_count,'''
new = '''    let enumerated = tile_digests.len() <= MAX_ENUMERATED_TILES;
    serde_json::to_string_pretty(&json!({
        "format": "MT2",
        "format_version": maps2_tile::FORMAT_VERSION,
        "levels": levels,
        "feature_count": feature_count,
        "tile_count": tile_digests.len(),
        // Named one by one while that is affordable; `null` past the
        // threshold, where `bounds` is the whole answer.
        "tiles": enumerated.then(|| tile_paths(tile_digests)),
        "tile_digests": enumerated.then(|| digest_map(tile_digests)),
        // Written whatever the size: this is the envelope the client
        // plans against, and until now only `carve` produced it.
        "bounds": carved_bounds(tile_digests),
        "package_sha256": package_sha256(tile_digests),
        "view": package_view(tile_digests),
        "height_tile_count": height_tile_count,'''
assert old in s
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8").write(s)
print("patched")
