//! Building a package larger than memory.
//!
//! The pipeline used to take every prepared feature as one slice and hand
//! back every tile as one `Vec`. For a carve that is the simplest thing
//! that works. For a planet neither end fits: something like 10^9
//! features go in and 10^8 tiles come out, and no machine holds either.
//!
//! A spool is the same build with the middle written down. Features are
//! pushed one at a time and land in one of many shard files, chosen by
//! the tile they belong to, so every part of a tile ends up in the same
//! shard. Draining reads one shard at a time, groups it, builds those
//! tiles and hands them on — so the memory a build needs is one shard,
//! and the number of shards is how the build is made to fit.
//!
//! Order does not matter to the result: [`crate::build_tile`] sorts a
//! tile's features itself, and the manifest sorts its tiles, so a spooled
//! build produces the same bytes as an in-memory one. That is a test, not
//! a hope — see `the_spool_builds_the_same_bytes_as_memory`.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use maps2_style::Class;
use maps2_tile::{FeatureDraft, TileError};
use maps2_units::{TileCoord, TileId};

use crate::{build_tile, BuildingHeight, DemGrid, MaterialClass, PreparedFeature, RoofType};

/// Everything that can go wrong writing a package through a spool.
#[derive(Debug)]
pub enum SpoolError {
    /// The spool's own files.
    Io(io::Error),
    /// A shard held bytes that are not a feature record.
    Corrupt(&'static str),
    /// A tile that cannot be encoded — the same limits `build_tile` has.
    Tile(TileError),
}

impl std::fmt::Display for SpoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "spool io: {error}"),
            Self::Corrupt(what) => write!(f, "spool record: {what}"),
            Self::Tile(error) => write!(f, "tile: {error:?}"),
        }
    }
}

impl From<io::Error> for SpoolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<TileError> for SpoolError {
    fn from(error: TileError) -> Self {
        Self::Tile(error)
    }
}

/// Where a feature is written down between the two passes.
pub struct FeatureSpool {
    shards: Vec<BufWriter<File>>,
    features: u64,
}

impl FeatureSpool {
    /// Opens `shards` files under `dir`, which the caller owns and is
    /// expected to remove.
    ///
    /// More shards means less memory per drain and more open files; the
    /// pipeline picks the number from how much ground it is about to
    /// build, not this module.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Io`] when the shard files cannot be created.
    pub fn new(dir: &Path, shards: usize) -> Result<Self, SpoolError> {
        std::fs::create_dir_all(dir)?;
        let files = (0..shards.max(1))
            .map(|index| {
                let path = dir.join(format!("shard-{index:05}.spool"));
                let file = File::options().create(true).read(true).write(true).truncate(true).open(path)?;
                Ok(BufWriter::new(file))
            })
            .collect::<Result<Vec<_>, io::Error>>()?;
        Ok(Self { shards: files, features: 0 })
    }

    /// How many features have been written.
    #[must_use]
    pub fn features(&self) -> u64 {
        self.features
    }

    /// Writes one feature to the shard its tile belongs to.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Io`] when the shard cannot be written.
    pub fn push(&mut self, feature: &PreparedFeature) -> Result<(), SpoolError> {
        let index = shard_of(feature.tile, self.shards.len());
        let mut record = Vec::new();
        encode(feature, &mut record);
        let shard = &mut self.shards[index];
        shard.write_all(&u32::try_from(record.len()).unwrap_or(u32::MAX).to_le_bytes())?;
        shard.write_all(&record)?;
        self.features += 1;
        Ok(())
    }

    /// Builds every tile, one shard's worth at a time, handing each to
    /// `sink` as soon as it exists — so the caller can write it out and
    /// let go of it.
    ///
    /// # Errors
    ///
    /// [`SpoolError`] from reading a shard back, decoding a record, or
    /// building a tile.
    pub fn drain<E>(
        mut self,
        terrain: &[DemGrid],
        mut sink: impl FnMut(TileId, Vec<u8>) -> Result<(), E>,
    ) -> Result<(), SpoolError>
    where
        SpoolError: From<E>,
    {
        let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        for shard in &mut self.shards {
            shard.flush()?;
            let file = shard.get_mut();
            file.seek(SeekFrom::Start(0))?;
            drain_shard(&read_shard(file)?, terrain, threads, &mut sink)?;
        }
        Ok(())
    }
}

/// One shard's tiles, built in batches and handed on in tile order.
fn drain_shard<E>(
    grouped: &HashMap<TileId, Vec<PreparedFeature>>,
    terrain: &[DemGrid],
    threads: usize,
    sink: &mut impl FnMut(TileId, Vec<u8>) -> Result<(), E>,
) -> Result<(), SpoolError>
where
    SpoolError: From<E>,
{
    let mut ids: Vec<TileId> = grouped.keys().copied().collect();
    ids.sort_by_key(|id| (id.z, id.x, id.y));
    for batch in ids.chunks(BATCH_TILES) {
        for (id, bytes) in build_batch(batch, grouped, terrain, threads)? {
            sink(id, bytes)?;
        }
    }
    Ok(())
}

/// Tiles built before any of them are handed on.
///
/// Small on purpose. Building a tile is the expensive half of ingest —
/// triangulation, line joins, label shaping — and worth spreading across
/// cores, but the whole point of the spool is not to hold tiles, so only
/// a batch is ever in the air: a hundred and fifty kilobytes each, times
/// this, times nothing else.
const BATCH_TILES: usize = 64;

/// One batch, built across threads, returned in the order it was asked
/// for.
///
/// The order matters more than it looks: it is what keeps a spooled build
/// producing the same package as a single-threaded one, whatever the
/// machine's core count. `std::thread::scope` rather than a thread pool
/// crate — the work is already batched, and a dependency for `chunks` and
/// `join` would be one to no purpose.
fn build_batch(
    batch: &[TileId],
    grouped: &HashMap<TileId, Vec<PreparedFeature>>,
    terrain: &[DemGrid],
    threads: usize,
) -> Result<Vec<(TileId, Vec<u8>)>, SpoolError> {
    let one = |id: &TileId| -> Result<(TileId, Vec<u8>), SpoolError> {
        let features: Vec<&PreparedFeature> = grouped[id].iter().collect();
        Ok(build_tile(*id, &features, terrain)?)
    };
    if threads <= 1 || batch.len() <= 1 {
        return batch.iter().map(one).collect();
    }
    let per_thread = batch.len().div_ceil(threads.min(batch.len()));
    std::thread::scope(|scope| {
        let handles: Vec<_> = batch
            .chunks(per_thread)
            .map(|chunk| scope.spawn(move || chunk.iter().map(&one).collect::<Result<Vec<_>, _>>()))
            .collect();
        let mut built = Vec::with_capacity(batch.len());
        for handle in handles {
            built.extend(handle.join().map_err(|_| SpoolError::Corrupt("build thread panicked"))??);
        }
        Ok(built)
    })
}

/// Which shard a tile's features live in.
///
/// Every part of a tile has to land together, so this is a function of
/// the tile alone. It mixes the coordinates rather than using them
/// directly: neighbouring tiles are built at the same time and would
/// otherwise pile into one shard while the rest sat empty.
fn shard_of(tile: TileId, shards: usize) -> usize {
    let mut hash = u64::from(tile.z)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(u64::from(tile.x).wrapping_mul(0xC2B2_AE3D_27D4_EB4F))
        .wrapping_add(u64::from(tile.y).wrapping_mul(0x1656_67B1_9E37_79F9));
    hash ^= hash >> 29;
    usize::try_from(hash % shards.max(1) as u64).unwrap_or_default()
}

fn read_shard(file: &mut File) -> Result<HashMap<TileId, Vec<PreparedFeature>>, SpoolError> {
    let mut reader = BufReader::new(file);
    let mut grouped: HashMap<TileId, Vec<PreparedFeature>> = HashMap::new();
    let mut length = [0_u8; 4];
    loop {
        match reader.read_exact(&mut length) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(SpoolError::Io(error)),
        }
        let mut record = vec![0_u8; u32::from_le_bytes(length) as usize];
        reader.read_exact(&mut record)?;
        let feature = decode(&record)?;
        grouped.entry(feature.tile).or_default().push(feature);
    }
    Ok(grouped)
}

// The record format is this module's own and never leaves it: no version,
// no magic, written and read by the same build. Little-endian throughout,
// like MT2, so the two are not a pair of different habits.

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_coords(out: &mut Vec<u8>, coords: &[TileCoord]) {
    put_u32(out, u32::try_from(coords.len()).unwrap_or(u32::MAX));
    for coord in coords {
        put_u16(out, coord.0);
        put_u16(out, coord.1);
    }
}

fn encode(feature: &PreparedFeature, out: &mut Vec<u8>) {
    out.push(feature.tile.z);
    put_u32(out, feature.tile.x);
    put_u32(out, feature.tile.y);
    put_u16(out, feature.class.code());
    out.extend_from_slice(&feature.feature.id.to_le_bytes());
    out.push(feature.feature.flags);
    out.push(feature.feature.rank);
    let name = feature.feature.name.as_bytes();
    put_u32(out, u32::try_from(name.len()).unwrap_or(u32::MAX));
    out.extend_from_slice(name);
    put_coords(out, &feature.feature.vertices);
    put_u32(out, u32::try_from(feature.feature.holes.len()).unwrap_or(u32::MAX));
    for hole in &feature.feature.holes {
        put_coords(out, hole);
    }
    let (tag, metres) = match feature.building_height {
        None => (0_u8, 0.0),
        Some(BuildingHeight::Explicit(metres)) => (1, metres),
        Some(BuildingHeight::Levels(metres)) => (2, metres),
        Some(BuildingHeight::Default(metres)) => (3, metres),
    };
    out.push(tag);
    out.extend_from_slice(&metres.to_le_bytes());
    out.push(feature.roof as u8);
    out.push(feature.material as u8);
    put_u16(out, feature.base_height_dm);
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], SpoolError> {
        let slice = self
            .bytes
            .get(self.at..self.at + count)
            .ok_or(SpoolError::Corrupt("record ends mid-field"))?;
        self.at += count;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, SpoolError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, SpoolError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, SpoolError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, SpoolError> {
        let bytes = self.take(8)?;
        let mut value = [0_u8; 8];
        value.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(value))
    }

    fn f32(&mut self) -> Result<f32, SpoolError> {
        let bytes = self.take(4)?;
        Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn coords(&mut self) -> Result<Vec<TileCoord>, SpoolError> {
        let count = self.u32()? as usize;
        (0..count).map(|_| Ok(TileCoord(self.u16()?, self.u16()?))).collect()
    }
}

fn decode(bytes: &[u8]) -> Result<PreparedFeature, SpoolError> {
    let mut cursor = Cursor { bytes, at: 0 };
    let tile = TileId { z: cursor.u8()?, x: cursor.u32()?, y: cursor.u32()? };
    let class = Class::from_code(cursor.u16()?).ok_or(SpoolError::Corrupt("unknown class"))?;
    let id = cursor.u64()?;
    let flags = cursor.u8()?;
    let rank = cursor.u8()?;
    let name_len = cursor.u32()? as usize;
    let name = std::str::from_utf8(cursor.take(name_len)?)
        .map_err(|_| SpoolError::Corrupt("name is not utf-8"))?
        .to_string();
    let vertices = cursor.coords()?;
    let hole_count = cursor.u32()? as usize;
    let holes = (0..hole_count).map(|_| cursor.coords()).collect::<Result<Vec<_>, _>>()?;
    let height_tag = cursor.u8()?;
    let metres = cursor.f32()?;
    let building_height = match height_tag {
        0 => None,
        1 => Some(BuildingHeight::Explicit(metres)),
        2 => Some(BuildingHeight::Levels(metres)),
        3 => Some(BuildingHeight::Default(metres)),
        _ => return Err(SpoolError::Corrupt("unknown height tag")),
    };
    let roof = roof_of(cursor.u8()?)?;
    let material = material_of(cursor.u8()?)?;
    let base_height_dm = cursor.u16()?;
    Ok(PreparedFeature {
        tile,
        class,
        feature: FeatureDraft { id, flags, rank, name, vertices, holes },
        building_height,
        roof,
        material,
        base_height_dm,
    })
}

fn roof_of(code: u8) -> Result<RoofType, SpoolError> {
    match code {
        0 => Ok(RoofType::Flat),
        1 => Ok(RoofType::Gabled),
        2 => Ok(RoofType::Hipped),
        3 => Ok(RoofType::Other),
        _ => Err(SpoolError::Corrupt("unknown roof")),
    }
}

fn material_of(code: u8) -> Result<MaterialClass, SpoolError> {
    match code {
        0 => Ok(MaterialClass::Unknown),
        1 => Ok(MaterialClass::Brick),
        2 => Ok(MaterialClass::Concrete),
        3 => Ok(MaterialClass::Stone),
        4 => Ok(MaterialClass::Glass),
        5 => Ok(MaterialClass::Metal),
        6 => Ok(MaterialClass::Wood),
        _ => Err(SpoolError::Corrupt("unknown material")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_tiles_with_terrains, prepare_feature, prepare_features};
    use maps2_units::Lonlat;

    /// A handful of features spread over several tiles and levels, with a
    /// building among them so the record's height, roof and material
    /// fields carry something other than their defaults.
    fn features() -> Vec<PreparedFeature> {
        let mut out = Vec::new();
        for (index, (lon, lat)) in [(-0.1278, 51.5074), (-0.1180, 51.5100), (0.0021, 51.4769)]
            .into_iter()
            .enumerate()
        {
            let id = 100 + index as u64;
            let square = [
                Lonlat { lon, lat },
                Lonlat { lon: lon + 0.0006, lat },
                Lonlat { lon: lon + 0.0006, lat: lat + 0.0004 },
                Lonlat { lon, lat: lat + 0.0004 },
                Lonlat { lon, lat },
            ];
            if let Some(feature) = prepare_feature(
                id,
                &[("building", "yes"), ("building:levels", "4"), ("roof:shape", "gabled"),
                  ("building:material", "brick"), ("name", "Tall Thing")],
                &square,
                16,
            ) {
                out.push(feature);
            }
            out.extend(prepare_features(
                id + 50,
                &[("highway", "primary"), ("name", "Long Road")],
                &[Lonlat { lon, lat }, Lonlat { lon: lon + 0.02, lat: lat + 0.01 }],
                12,
            ));
        }
        assert!(out.len() > 3, "the fixture has something to spread around");
        out
    }

    #[test]
    fn a_record_survives_the_round_trip_whole() {
        for feature in features() {
            let mut bytes = Vec::new();
            encode(&feature, &mut bytes);
            let back = decode(&bytes).expect("decodes");
            assert_eq!(back.tile, feature.tile);
            assert_eq!(back.class, feature.class);
            assert_eq!(back.feature, feature.feature);
            assert_eq!(back.building_height, feature.building_height);
            assert_eq!(back.roof, feature.roof);
            assert_eq!(back.material, feature.material);
            assert_eq!(back.base_height_dm, feature.base_height_dm);
        }
    }

    #[test]
    fn a_truncated_record_is_an_error_not_a_panic() {
        let mut bytes = Vec::new();
        encode(&features()[0], &mut bytes);
        for cut in [0, 1, 5, bytes.len() / 2, bytes.len() - 1] {
            assert!(matches!(decode(&bytes[..cut]), Err(SpoolError::Corrupt(_))), "cut at {cut}");
        }
    }

    /// The claim the whole module rests on: writing the middle to disk
    /// does not change the output.
    #[test]
    fn the_spool_builds_the_same_bytes_as_memory() {
        let prepared = features();
        let expected = build_tiles_with_terrains(&prepared, &[]).expect("in-memory build");

        let dir = tempfile::tempdir().expect("scratch");
        let mut spooled: Vec<(TileId, Vec<u8>)> = Vec::new();
        // Enough shards that the tiles genuinely land in different ones.
        let count = build_tiles_spooled_for_test(&prepared, dir.path(), 8, &mut spooled);

        assert_eq!(count, prepared.len() as u64);
        spooled.sort_by_key(|(id, _)| (id.z, id.x, id.y));
        let mut expected_sorted = expected;
        expected_sorted.sort_by_key(|(id, _)| (id.z, id.x, id.y));
        assert_eq!(spooled, expected_sorted);
    }

    /// The same features, spread over one shard and over many, come out
    /// the same — so how a build is made to fit cannot change what it
    /// builds.
    #[test]
    fn the_number_of_shards_does_not_reach_the_output() {
        let prepared = features();
        let build = |shards: usize| {
            let dir = tempfile::tempdir().expect("scratch");
            let mut out = Vec::new();
            build_tiles_spooled_for_test(&prepared, dir.path(), shards, &mut out);
            out.sort_by_key(|(id, _)| (id.z, id.x, id.y));
            out
        };
        assert_eq!(build(1), build(64));
    }

    fn build_tiles_spooled_for_test(
        prepared: &[PreparedFeature],
        dir: &Path,
        shards: usize,
        out: &mut Vec<(TileId, Vec<u8>)>,
    ) -> u64 {
        crate::build_tiles_spooled(
            prepared.iter().cloned(),
            &[],
            dir,
            shards,
            |id, bytes| -> Result<(), SpoolError> {
                out.push((id, bytes));
                Ok(())
            },
        )
        .expect("spooled build")
    }

    /// However many cores the machine has, the package is the same. The
    /// batch is built across threads and collected back in the order it
    /// was asked for; if that ordering ever slipped, this is what would
    /// notice.
    #[test]
    fn the_thread_count_does_not_reach_the_output() {
        let prepared = features();
        let mut grouped: HashMap<TileId, Vec<PreparedFeature>> = HashMap::new();
        for feature in prepared {
            grouped.entry(feature.tile).or_default().push(feature);
        }
        let mut ids: Vec<TileId> = grouped.keys().copied().collect();
        ids.sort_by_key(|id| (id.z, id.x, id.y));

        let alone = build_batch(&ids, &grouped, &[], 1).expect("single-threaded");
        for threads in [2, 3, 8, 64] {
            assert_eq!(
                build_batch(&ids, &grouped, &[], threads).expect("threaded"),
                alone,
                "with {threads} threads",
            );
        }
    }

    #[test]
    fn every_part_of_a_tile_lands_in_one_shard() {
        let tile = TileId { z: 14, x: 8187, y: 5448 };
        let shards = 32;
        let first = shard_of(tile, shards);
        assert_eq!(shard_of(tile, shards), first, "the same tile, the same shard");
        // Neighbours are spread rather than piled together: they are built
        // at the same time, and one hot shard is the memory this module
        // exists to bound.
        let neighbours: std::collections::HashSet<usize> = (0..8)
            .map(|step| shard_of(TileId { z: 14, x: 8187 + step, y: 5448 }, shards))
            .collect();
        assert!(neighbours.len() > 4, "eight neighbours landed in {} shards", neighbours.len());
    }
}
