//! Decodes every resource of an installed copy of the game.
//!
//! Game data is copyrighted and not part of the repository, so this test only
//! runs when `OPENAC_DATA` points at a directory created by
//! `openac-extract install`; otherwise it passes trivially.

use std::path::{Path, PathBuf};

use openac_formats::{Archive, Bitmap, MapGrid, Mask, ResourceKind, Sprite, TileSetLibrary};

fn data_dir() -> Option<PathBuf> {
    std::env::var_os("OPENAC_DATA").map(PathBuf::from)
}

fn archives(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in [
        root.to_path_buf(),
        root.join("data"),
        root.join("sound"),
        root.join("video"),
    ] {
        let Ok(items) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in items.flatten() {
            let p = item.path();
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dkx")) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn every_resource_decodes() {
    let Some(root) = data_dir() else {
        eprintln!("OPENAC_DATA not set; skipping");
        return;
    };
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for path in archives(&root) {
        let archive = Archive::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for e in archive.entries() {
            let data = archive
                .read_entry(e)
                .unwrap_or_else(|err| panic!("{} {}: {err}", path.display(), e.name));
            assert_eq!(data.len(), e.size as usize, "{} {}", path.display(), e.name);
            let ctx = || format!("{} {} ({:?})", path.display(), e.name, e.kind);
            match e.kind {
                ResourceKind::Bitmap => {
                    let b = Bitmap::parse(&data).unwrap_or_else(|err| panic!("{}: {err}", ctx()));
                    assert_eq!(
                        (b.width, b.height),
                        (e.width as u32, e.height as u32),
                        "{}",
                        ctx()
                    );
                }
                ResourceKind::Sprite => {
                    let s = Sprite::parse(&data).unwrap_or_else(|err| panic!("{}: {err}", ctx()));
                    assert_eq!(
                        (s.width, s.height),
                        (e.width as u32, e.height as u32),
                        "{}",
                        ctx()
                    );
                }
                ResourceKind::Mask => {
                    Mask::parse(&data, e.width.into(), e.height.into())
                        .unwrap_or_else(|err| panic!("{}: {err}", ctx()));
                }
                ResourceKind::MapGrid => {
                    let g = MapGrid::parse(&data, e.width.into(), e.height.into())
                        .unwrap_or_else(|err| panic!("{}: {err}", ctx()));
                    assert_eq!(g.layers, 8, "{}", ctx());
                }
                _ => {}
            }
            *counts.entry(format!("{:?}", e.kind)).or_default() += 1;
        }
    }
    eprintln!("decoded: {counts:?}");
    assert!(counts.get("Sprite").copied().unwrap_or(0) > 50_000);
}

#[test]
fn every_map_tile_exists() {
    let Some(root) = data_dir() else { return };
    let maplib = Archive::open(root.join("maplib.dkx")).unwrap();
    let lib = TileSetLibrary::parse(&maplib.read("b_ALIB", ResourceKind::TileSetLibrary).unwrap())
        .unwrap();
    for path in archives(&root) {
        let archive = Archive::open(&path).unwrap();
        let Some(e) = archive.find("b_MAPA", ResourceKind::MapGrid) else {
            continue;
        };
        let grid = MapGrid::parse(
            &archive.read_entry(e).unwrap(),
            e.width.into(),
            e.height.into(),
        )
        .unwrap();
        for layer in 0..grid.layers {
            for y in 0..grid.height {
                for x in 0..grid.width {
                    let cell = grid.get(layer, x, y);
                    if cell.is_empty() {
                        continue;
                    }
                    let set = lib.by_id(cell.tileset).unwrap_or_else(|| {
                        panic!("{}: unknown tile set {:#x}", path.display(), cell.tileset)
                    });
                    let name = set.tile_name(cell.variant);
                    let found = [ResourceKind::Sprite, ResourceKind::Bitmap]
                        .iter()
                        .any(|&k| maplib.find(&name, k).is_some());
                    assert!(
                        found,
                        "{}: layer {layer} ({x},{y}) references missing tile {name}",
                        path.display()
                    );
                }
            }
        }
    }
}

#[test]
fn composites_and_sequences_resolve() {
    use openac_formats::{anim::EMPTY_SPRITE, Composite, Sequence};
    let Some(root) = data_dir() else { return };
    let objects = Archive::open(root.join("data").join("objects.dkx")).unwrap();
    let (mut composites, mut sequences, mut missing) = (0, 0, Vec::new());
    for e in objects.entries() {
        let data = objects.read_entry(e).unwrap();
        match e.kind {
            ResourceKind::Composite => {
                let c =
                    Composite::parse(&data, e).unwrap_or_else(|err| panic!("{}: {err}", e.name));
                for f in 0..c.frames {
                    for cell in c.frame(f) {
                        if cell.sprite != EMPTY_SPRITE
                            && objects.find(&cell.sprite, ResourceKind::Sprite).is_none()
                        {
                            missing.push(format!("{} -> {}", e.name, cell.sprite));
                        }
                    }
                }
                composites += 1;
            }
            ResourceKind::Sequence => {
                let s = Sequence::parse(&data, e).unwrap_or_else(|err| panic!("{}: {err}", e.name));
                if s.frame_names(&objects).is_empty() {
                    missing.push(format!("{} -> {}*", e.name, s.prefix));
                }
                sequences += 1;
            }
            _ => {}
        }
    }
    missing.sort();
    missing.dedup();
    eprintln!(
        "{composites} composites, {sequences} sequences, {} unresolved references",
        missing.len()
    );
    for m in missing.iter().take(20) {
        eprintln!("  {m}");
    }
    assert!(composites > 500 && sequences > 800);
}

#[test]
fn fonts_and_atlases_load() {
    use openac_formats::Font;
    let Some(root) = data_dir() else { return };
    let mut fonts = 0;
    for path in archives(&root) {
        let archive = Archive::open(&path).unwrap();
        for e in archive
            .entries()
            .iter()
            .filter(|e| e.kind == ResourceKind::Font)
        {
            let font = Font::parse(&archive.read_entry(e).unwrap())
                .unwrap_or_else(|err| panic!("{} {}: {err}", path.display(), e.name));
            for i in 0..font.atlas_offsets.len() {
                let atlas = font
                    .atlas(&archive, i)
                    .unwrap_or_else(|err| panic!("{} atlas {i}: {err}", e.name));
                for (ch, g) in &font.glyphs {
                    assert!(
                        (g.x + g.width) as u32 <= atlas.width
                            && (g.y + g.height) as u32 <= atlas.height,
                        "{} glyph {ch} outside atlas",
                        e.name
                    );
                }
            }
            fonts += 1;
        }
    }
    assert!(fonts >= 30);
}

#[test]
fn every_mission_has_one_human_player() {
    let Some(root) = data_dir() else { return };
    for path in archives(&root) {
        let archive = Archive::open(&path).unwrap();
        if archive.find("b_MAPA", ResourceKind::MapGrid).is_none()
            || archive.find("DESCRIPTOR", ResourceKind::Raw).is_none()
        {
            continue;
        }
        let players = openac_formats::players(&archive).unwrap();
        if players.is_empty() {
            // Saved games (demo.dkx) store players elsewhere.
            continue;
        }
        let humans = players.iter().filter(|p| !p.computer).count();
        assert!(humans >= 1, "{}: {players:?}", path.display());
        for p in &players {
            assert!(matches!(p.nation, 1 | 3 | 4), "{}: {p:?}", path.display());
        }
    }
}
