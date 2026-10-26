//! Finding the game data without command-line options: the folder given
//! (`--data`, `OPENAC_DATA`), the one remembered from last time, a
//! `gamedata` folder here or next to the program; failing those, the
//! player is asked for the Ancient Conquest CD (or a mounted image of it)
//! or the game's install folder. Data from a CD is installed into the
//! user's data folder first. The choice is remembered.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use openac_formats::install::{install, is_cd, is_installed};
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

const TITLE: &str = "OpenAC";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Config {
    data: Option<PathBuf>,
}

fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("OpenAC").join("config.json"))
}

fn remembered() -> Option<PathBuf> {
    let text = std::fs::read_to_string(config_path()?).ok()?;
    serde_json::from_str::<Config>(&text).ok()?.data
}

fn remember(data: &Path) {
    let Some(path) = config_path() else { return };
    let config = Config {
        data: Some(data.to_path_buf()),
    };
    if let (Some(dir), Ok(text)) = (path.parent(), serde_json::to_string_pretty(&config)) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(&path, text);
    }
}

/// Where data installed from a CD goes.
fn install_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("OpenAC")
        .join("gamedata")
}

/// Places a `gamedata` folder may be in: here, next to the program, and in
/// a macOS application bundle's resources.
fn nearby() -> Vec<PathBuf> {
    let mut places = vec![PathBuf::from("gamedata")];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        places.push(dir.join("gamedata"));
        places.push(dir.join("../Resources/gamedata"));
    }
    places
}

/// The game data folder, asking the player for it if need be; `None` if
/// they chose not to.
pub fn data_dir(given: Option<PathBuf>) -> Result<Option<PathBuf>> {
    if let Some(dir) = given {
        return Ok(Some(dir));
    }
    if let Some(dir) = remembered().filter(|d| is_installed(d)) {
        return Ok(Some(dir));
    }
    if let Some(dir) = nearby().into_iter().find(|d| is_installed(d)) {
        return Ok(Some(dir));
    }
    let welcome = MessageDialog::new()
        .set_title(TITLE)
        .set_level(MessageLevel::Info)
        .set_description(
            "Welcome to OpenAC.\n\nIt plays Ancient Conquest with the original game's data. \
             Choose the Ancient Conquest CD (or a mounted image of it), or the folder the \
             game is installed in.",
        )
        .set_buttons(MessageButtons::OkCancel)
        .show();
    if welcome != MessageDialogResult::Ok {
        return Ok(None);
    }
    loop {
        let mut dialog =
            rfd::FileDialog::new().set_title("Choose the Ancient Conquest CD or install folder");
        if cfg!(target_os = "macos") {
            dialog = dialog.set_directory("/Volumes");
        }
        let Some(folder) = dialog.pick_folder() else {
            return Ok(None);
        };
        if is_installed(&folder) {
            remember(&folder);
            return Ok(Some(folder));
        }
        if is_cd(&folder) {
            let dest = install_dir();
            let go = MessageDialog::new()
                .set_title(TITLE)
                .set_level(MessageLevel::Info)
                .set_description(format!(
                    "OpenAC will copy the game's data (about 250 MB) from the CD to\n{}\n\n\
                     This takes a minute or two; the game starts when it is done.",
                    dest.display()
                ))
                .set_buttons(MessageButtons::OkCancel)
                .show();
            if go != MessageDialogResult::Ok {
                return Ok(None);
            }
            install(&folder, &dest, true, |line| eprintln!("{line}"))
                .with_context(|| format!("installing the game data to {}", dest.display()))?;
            remember(&dest);
            return Ok(Some(dest));
        }
        // An install of the original that left its data on the CD.
        let hint = if matches!(
            openac_formats::install::find_ci(&folder, "maplib.dkx"),
            Ok(Some(_))
        ) {
            "This installation keeps the game's data on the CD. Please choose the CD instead."
        } else {
            "This folder does not hold Ancient Conquest. Please choose the CD (or a mounted \
             image of it), or the folder the game is installed in."
        };
        MessageDialog::new()
            .set_title(TITLE)
            .set_level(MessageLevel::Warning)
            .set_description(hint)
            .set_buttons(MessageButtons::Ok)
            .show();
    }
}

/// Shows an error in a dialog (the program may have no terminal).
pub fn show_error(error: &anyhow::Error) {
    MessageDialog::new()
        .set_title(TITLE)
        .set_level(MessageLevel::Error)
        .set_description(format!("{error:#}"))
        .set_buttons(MessageButtons::Ok)
        .show();
}
