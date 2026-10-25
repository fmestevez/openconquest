//! Terrain maps: the per-mission cell grid (`b_MAPA`) and the tile set library
//! (`b_ALIB` / `s_ALIB` in MAPLIB).

use crate::bytes::latin1_cstr;
use crate::{invalid, Result};

/// Size in pixels of a full-size (`b_`) terrain tile.
pub const TILE_SIZE: u32 = 64;
/// Size in pixels of a minimap (`s_`) terrain tile.
pub const MINI_TILE_SIZE: u32 = 8;

const TILESET_RECORD: usize = 211;

/// One tile set of the library: a family of numbered tiles sharing a name
/// prefix, e.g. `b_isl_bkg00_` -> `b_isl_bkg00_001`, `b_isl_bkg00_002`, ...
#[derive(Debug, Clone)]
pub struct TileSet {
    /// Index of the record within the library.
    pub index: usize,
    /// Identifier referenced by map cells.
    pub id: u8,
    /// Resource name prefix of the tiles.
    pub prefix: String,
    /// The full record, for fields not yet understood.
    pub raw: Vec<u8>,
}

impl TileSet {
    /// Resource name of tile `variant` of this set.
    pub fn tile_name(&self, variant: u8) -> String {
        format!("{}{:03}", self.prefix, variant)
    }
}

#[derive(Debug, Clone, Default)]
pub struct TileSetLibrary {
    pub sets: Vec<TileSet>,
}

impl TileSetLibrary {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() % TILESET_RECORD != 0 {
            return Err(invalid(
                "tile set library",
                format!("size {} is not a multiple of {TILESET_RECORD}", data.len()),
            ));
        }
        let sets = data
            .chunks_exact(TILESET_RECORD)
            .enumerate()
            .map(|(index, rec)| TileSet {
                index,
                id: rec[0],
                prefix: latin1_cstr(&rec[8..24]),
                raw: rec.to_vec(),
            })
            .collect();
        Ok(Self { sets })
    }

    /// The tile set referenced by map cells with the given id. When several
    /// records share an id, the first one wins.
    pub fn by_id(&self, id: u8) -> Option<&TileSet> {
        self.sets.iter().find(|s| s.id == id)
    }
}

/// A reference from a map cell to a terrain tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileRef {
    pub variant: u8,
    pub tileset: u8,
}

impl TileRef {
    pub fn is_empty(self) -> bool {
        self.variant == 0 && self.tileset == 0
    }
}

/// The layered terrain grid of a mission (`b_MAPA`, resource kind 17).
///
/// The grid holds `layers` planes of `width * height` little-endian u16
/// cells; the low byte is the tile variant, the high byte the tile set id.
/// Layer 0 is the base terrain (sea, shelf, island ground); higher layers
/// overlay shoals, vegetation, hills and decorations. Cell `(0, 0)` empty
/// means nothing is drawn on that layer.
#[derive(Debug, Clone)]
pub struct MapGrid {
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    cells: Vec<TileRef>,
}

impl MapGrid {
    pub fn parse(data: &[u8], width: u32, height: u32) -> Result<Self> {
        let plane = (width * height * 2) as usize;
        if plane == 0 || data.len() % plane != 0 {
            return Err(invalid(
                "map grid",
                format!("{} bytes for a {width}x{height} grid", data.len()),
            ));
        }
        let cells = data
            .chunks_exact(2)
            .map(|c| TileRef {
                variant: c[0],
                tileset: c[1],
            })
            .collect();
        Ok(Self {
            width,
            height,
            layers: (data.len() / plane) as u32,
            cells,
        })
    }

    pub fn get(&self, layer: u32, x: u32, y: u32) -> TileRef {
        self.cells[((layer * self.height + y) * self.width + x) as usize]
    }

    /// Size of the map in pixels at full tile size.
    pub fn pixel_size(&self) -> (u32, u32) {
        (self.width * TILE_SIZE, self.height * TILE_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_layers() {
        let mut data = vec![0u8; 2 * 2 * 2 * 3];
        data[2 * 2 * 2 + 2] = 5; // layer 1, cell (1, 0)
        data[2 * 2 * 2 + 3] = 7;
        let g = MapGrid::parse(&data, 2, 2).unwrap();
        assert_eq!(g.layers, 3);
        assert_eq!(
            g.get(1, 1, 0),
            TileRef {
                variant: 5,
                tileset: 7
            }
        );
        assert!(g.get(0, 1, 0).is_empty());
    }

    #[test]
    fn tile_names() {
        let mut rec = vec![0u8; TILESET_RECORD];
        rec[0] = 7;
        rec[8..20].copy_from_slice(b"b_isl_bkg00_");
        let lib = TileSetLibrary::parse(&rec).unwrap();
        assert_eq!(lib.by_id(7).unwrap().tile_name(1), "b_isl_bkg00_001");
    }
}
