# MT2 v6 TL;DR — for agents

Full spec: `tile-format.md` (RU). This is the English token-efficient summary.

## Constants

* `MAGIC = b"MT2\0"` `crates/maps2-tile/src/lib.rs:48`
* `FORMAT_VERSION = 5` `lib.rs:49` (readers accept 1-5)
* `TILE_EXTENT = 65536` `maps2-units/src/lib.rs:18` — <10cm at z16
* `RASTER_CLASS_BASE = 0xFF00` `lib.rs:58` — class >=0xFF00 is raster

## Header (LE, 20 + 10*count bytes) `lib.rs:16`

Offset 0:4 magic, 4:2 version, 6:1 z, 7:1 reserved, 8:4 x, 12:4 y, 16:2 section_count, 18:2 reserved, 20: table.
Table entry: `class u16, offset u32, len u32` — offset from payload base `20+10*count`.

## Section dispatch

* `class < 0xFF00` vector (classes 0-12 from `maps2-style/src/lib.rs:1`: 0 land,1 water,2 park,3 building,4-10 roads,11 poi,12 label)
* `class == 0xFF00` heights raster: 256*256 u16 LE =131072 bytes, `metres = value - 11000` `heights.rs:1`
* `class == 0xFF01` same raster packed (Paeth over u16 + hi/lo byte planes + deflate, ~3.7x) `heights.rs:1` `pack`/`unpack`; ingest writes this since v6, a tile carries one or the other

## Vector payload

`feature_count u16` then per feature:
`id u64, flags u8, rank u8, base_dm u16, top_dm u16, roof u8, material u8, name_len u16, name utf8, vertex_count u16, x u16 y u16, (dx dy zigzag-varint)*`

`rank` 0=highest. `roof`:0 flat,1 gabled,2 hipped,3 other. `material`:0 unknown,1 brick,2 concrete,3 stone,4 glass,5 metal,6 wood.
Zigzag: `enc(v)=(v<<1)^(v>>31)`, varint 7b continuation, >5 bytes = BadVarint.

## Version history

1: id u32, no building. 2: base/top/roof. 3: holes. 4: id u64. 5: material (reader fills Unknown for v2-4).

## Error contract `lib.rs:74`

Never panics. `TooShort|BadMagic|UnsupportedVersion|SectionOutOfBounds|Truncated|BadVarint|DeltaOutOfRange|BadText|BadBuilding|TooLarge|EmptyGeometry`.

## When to edit

Bump `FORMAT_VERSION` `lib.rs:49` → update `tile-format.md` → regenerate fixtures → update golden hash `maps2-fixtures/src/lib.rs:382`.
