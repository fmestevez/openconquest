//! Installing the game data from the CD: the plain files of its `data`,
//! `sound` and `video` folders, and the files packed in its installer
//! (`SETUP.DAT`: maps, missions, interface art).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::installer::Installer;

/// Finds a file in `dir` by case-insensitive name.
pub fn find_ci(dir: &Path, name: &str) -> io::Result<Option<PathBuf>> {
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

/// Whether `dir` is the Ancient Conquest CD (or a copy of it).
pub fn is_cd(dir: &Path) -> bool {
    matches!(find_ci(dir, "setup.dat"), Ok(Some(_)))
}

/// Whether `dir` holds installed game data: the original game's install
/// folder, or one made by `install`.
pub fn is_installed(dir: &Path) -> bool {
    matches!(find_ci(dir, "maplib.dkx"), Ok(Some(_)))
        && matches!(find_ci(&dir.join("data"), "objects.dkx"), Ok(Some(_)))
}

/// Installs the game data from the CD at `cd` into `dest`, telling
/// `progress` each file it copies or unpacks. Without `video`, the
/// videos (about 110 MB) are left out.
pub fn install(
    cd: &Path,
    dest: &Path,
    video: bool,
    mut progress: impl FnMut(&str),
) -> io::Result<()> {
    let setup = find_ci(cd, "setup.dat")?.ok_or_else(|| {
        io::Error::other(format!(
            "{} does not look like the Ancient Conquest CD (no SETUP.DAT)",
            cd.display()
        ))
    })?;
    fs::create_dir_all(dest)?;
    let mut dirs = vec!["data", "sound"];
    if video {
        dirs.push("video");
    }
    for dir in dirs {
        let src_dir = find_ci(cd, dir)?
            .ok_or_else(|| io::Error::other(format!("missing directory {dir} on the CD")))?;
        let dst_dir = dest.join(dir);
        fs::create_dir_all(&dst_dir)?;
        for item in fs::read_dir(&src_dir)? {
            let item = item?;
            if !item.file_type()?.is_file() {
                continue;
            }
            let name = item.file_name().to_string_lossy().to_ascii_lowercase();
            progress(&format!("copy    {dir}/{name}"));
            fs::copy(item.path(), dst_dir.join(&name)).map_err(|e| {
                io::Error::new(e.kind(), format!("copying {}: {e}", item.path().display()))
            })?;
        }
    }
    let data = fs::read(&setup)?;
    let installer = Installer::parse(&data).map_err(io::Error::other)?;
    for file in installer.files() {
        // Placeholder entries such as "*EMPTY" carry no data.
        if file.name.starts_with('*') || file.name.contains(['/', '\\']) {
            continue;
        }
        let bytes = installer
            .extract(file)
            .map_err(|e| io::Error::other(format!("unpacking {}: {e}", file.name)))?;
        let name = file.name.to_ascii_lowercase();
        progress(&format!("unpack  {name} ({} bytes)", bytes.len()));
        fs::write(dest.join(&name), bytes)?;
    }
    Ok(())
}
