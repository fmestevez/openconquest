//! `openac-extract`: installs the game data from an Ancient Conquest CD and
//! inspects archives.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use openac_formats::installer::Installer;
use openac_formats::{Archive, Bitmap, Mask, Palette, ResourceKind, Sprite};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Copy the game data from the CD (or a mounted ISO) into a data directory.
    Install {
        /// Root of the CD, e.g. /Volumes/ANCIENT_CONQUEST or D:\
        cd: PathBuf,
        /// Destination data directory.
        dest: PathBuf,
        /// Skip the intro/outro videos (about 110 MB).
        #[arg(long)]
        no_video: bool,
    },
    /// List the entries of a .DKX/.DKD archive.
    List { archive: PathBuf },
    /// Export the entries of an archive: images as PNG, everything else raw.
    Dump {
        archive: PathBuf,
        out: PathBuf,
        /// Palette for sprites: `<archive>:<bitmap name>`. Defaults to the
        /// `PALETTE` bitmap of maplib next to the archive or in its parent.
        #[arg(long)]
        palette: Option<String>,
        /// Only dump entries whose name contains this substring (case-insensitive).
        #[arg(long)]
        filter: Option<String>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Install { cd, dest, no_video } => install(&cd, &dest, no_video),
        Command::List { archive } => list(&archive),
        Command::Dump {
            archive,
            out,
            palette,
            filter,
        } => dump(&archive, &out, palette.as_deref(), filter.as_deref()),
    }
}

/// Finds a file in `dir` by case-insensitive name.
fn find_ci(dir: &Path, name: &str) -> Result<Option<PathBuf>> {
    if !dir.is_dir() {
        return Ok(None);
    }
    for item in fs::read_dir(dir)? {
        let item = item?;
        if item
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
        {
            return Ok(Some(item.path()));
        }
    }
    Ok(None)
}

fn install(cd: &Path, dest: &Path, no_video: bool) -> Result<()> {
    let setup = find_ci(cd, "setup.dat")?.with_context(|| {
        format!(
            "{} does not look like the Ancient Conquest CD (no SETUP.DAT)",
            cd.display()
        )
    })?;
    fs::create_dir_all(dest)?;

    // Plain files from the CD subdirectories.
    let mut dirs = vec!["data", "sound"];
    if !no_video {
        dirs.push("video");
    }
    for dir in dirs {
        let Some(src_dir) = find_ci(cd, dir)? else {
            bail!("missing directory {dir} on the CD");
        };
        let dst_dir = dest.join(dir);
        fs::create_dir_all(&dst_dir)?;
        for item in fs::read_dir(&src_dir)? {
            let item = item?;
            if !item.file_type()?.is_file() {
                continue;
            }
            let name = item.file_name().to_string_lossy().to_ascii_lowercase();
            let dst = dst_dir.join(&name);
            println!("copy    {dir}/{name}");
            fs::copy(item.path(), &dst)
                .with_context(|| format!("copying {}", item.path().display()))?;
        }
    }

    // Files packed in the installer: maps, missions, interface art, executables.
    let data = fs::read(&setup)?;
    let installer = Installer::parse(&data)?;
    for file in installer.files() {
        // Placeholder entries such as "*EMPTY" carry no data.
        if file.name.starts_with('*') || file.name.contains(['/', '\\']) {
            continue;
        }
        let bytes = installer
            .extract(file)
            .with_context(|| format!("unpacking {}", file.name))?;
        let name = file.name.to_ascii_lowercase();
        println!("unpack  {name} ({} bytes)", bytes.len());
        fs::write(dest.join(&name), bytes)?;
    }
    println!("Game data installed to {}", dest.display());
    Ok(())
}

fn list(path: &Path) -> Result<()> {
    let archive = Archive::open(path)?;
    println!(
        "{:<32} {:>18} {:>9} {:>9} {:>4} {:>11}",
        "name", "kind", "size", "stored", "w", "h"
    );
    for e in archive.entries() {
        println!(
            "{:<32} {:>18} {:>9} {:>9} {:>4} {:>11}",
            e.name,
            format!("{:?}", e.kind),
            e.size,
            if e.compressed {
                e.stored_size.to_string()
            } else {
                "-".into()
            },
            e.width,
            e.height
        );
    }
    eprintln!("{} entries", archive.entries().len());
    Ok(())
}

fn default_palette(archive: &Path) -> Result<Palette> {
    let dir = archive.parent().unwrap_or(Path::new("."));
    for d in [dir, dir.parent().unwrap_or(dir)] {
        if let Some(p) = find_ci(d, "maplib.dkx")? {
            let a = Archive::open(p)?;
            return Ok(Palette::from_bitmap(
                &a.read("PALETTE", ResourceKind::Bitmap)?,
            )?);
        }
    }
    bail!("could not find maplib.dkx for the default palette; pass --palette")
}

fn dump(path: &Path, out: &Path, palette: Option<&str>, filter: Option<&str>) -> Result<()> {
    let archive = Archive::open(path)?;
    let palette = match palette {
        Some(spec) => {
            let (arc, name) = spec
                .rsplit_once(':')
                .context("--palette must be <archive>:<name>")?;
            Palette::from_bitmap(&Archive::open(arc)?.read(name, ResourceKind::Bitmap)?)?
        }
        None => default_palette(path)?,
    };
    let filter = filter.map(str::to_ascii_lowercase);
    fs::create_dir_all(out)?;
    let mut count = 0;
    for e in archive.entries() {
        if filter
            .as_ref()
            .is_some_and(|f| !e.name.to_ascii_lowercase().contains(f))
        {
            continue;
        }
        let data = archive.read_entry(e)?;
        let stem = format!("{}.{}", sanitize(&e.name), e.kind.tag());
        let result = match e.kind {
            ResourceKind::Bitmap => Bitmap::parse(&data).map(|b| {
                let rgba = b
                    .pixels
                    .iter()
                    .flat_map(|&i| rgba_px(&b.palette, i))
                    .collect();
                (b.width, b.height, rgba)
            }),
            ResourceKind::Sprite => Sprite::parse(&data).map(|s| {
                let mut rgba = vec![0u8; (s.width * s.height * 4) as usize];
                for y in 0..s.height {
                    for x in 0..s.width {
                        if let Some(i) = s.pixel(x, y) {
                            let o = ((y * s.width + x) * 4) as usize;
                            rgba[o..o + 4].copy_from_slice(&rgba_px(&palette, i));
                        }
                    }
                }
                (s.width, s.height, rgba)
            }),
            ResourceKind::Mask => Mask::parse(&data, e.width.into(), e.height.into()).map(|m| {
                let mut rgba = Vec::with_capacity((m.width * m.height * 4) as usize);
                for y in 0..m.height {
                    for x in 0..m.width {
                        let v = if m.get(x, y) { 255 } else { 0 };
                        rgba.extend_from_slice(&[v, v, v, 255]);
                    }
                }
                (m.width, m.height, rgba)
            }),
            _ => {
                fs::write(out.join(format!("{stem}.bin")), &data)?;
                count += 1;
                continue;
            }
        };
        match result {
            Ok((w, h, rgba)) => write_png(&out.join(format!("{stem}.png")), w, h, &rgba)?,
            Err(err) => {
                eprintln!("warning: {}: {err}; writing raw data", e.name);
                fs::write(out.join(format!("{stem}.bin")), &data)?;
            }
        }
        count += 1;
    }
    println!("dumped {count} entries to {}", out.display());
    Ok(())
}

fn rgba_px(p: &Palette, i: u8) -> [u8; 4] {
    let c = p.0[i as usize];
    [c.r, c.g, c.b, 255]
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_-.$".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<()> {
    let file = fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}
