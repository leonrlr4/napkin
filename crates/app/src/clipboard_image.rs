//! Reads the regular Wayland clipboard through `wl-clipboard-rs` (the data-control protocols),
//! which unlike egui's own clipboard access can see image data. Canvas pastes go through here;
//! text edits keep egui's paste.

use std::io::Read;

use wl_clipboard_rs::paste::{self, ClipboardType, MimeType, Seat};

/// Image formats in preference order.
const IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];
const SVG_TYPE: &str = "image/svg+xml";
const TEXT_TYPES: [&str; 3] = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"];

/// Upper bound on bytes taken from the clipboard; `image_file::prepare` rejects anything it
/// cannot shrink under its own limit, this only stops a runaway offer from filling memory.
const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub enum ClipboardContent {
    Image(Vec<u8>),
    Svg,
    Text(String),
    Empty,
}

/// The MIME type to read from `offered`: the first of [`IMAGE_TYPES`], else SVG, else a text
/// type. Images win over text because copying an image from a browser also offers its URL or
/// markup as text.
pub fn choose(offered: &[String]) -> Option<&'static str> {
    let has = |wanted: &str| offered.iter().any(|offered| offered == wanted);
    IMAGE_TYPES
        .iter()
        .chain(std::iter::once(&SVG_TYPE))
        .chain(TEXT_TYPES.iter())
        .find(|wanted| has(wanted))
        .copied()
}

/// Reads the clipboard. `Err` when data-control is unavailable (no such protocol, or no
/// connection), so the caller can fall back to egui's text paste; read failures after the
/// clipboard was reached are logged and reported as [`ClipboardContent::Empty`].
pub fn read() -> Result<ClipboardContent, String> {
    let offered = match paste::get_mime_types_ordered(ClipboardType::Regular, Seat::Unspecified) {
        Ok(offered) => offered,
        Err(paste::Error::ClipboardEmpty | paste::Error::NoMimeType) => {
            return Ok(ClipboardContent::Empty);
        }
        Err(
            error @ (paste::Error::MissingProtocol { .. }
            | paste::Error::WaylandConnection(_)
            | paste::Error::SocketOpenError(_)
            | paste::Error::NoSeats),
        ) => return Err(error.to_string()),
        Err(error) => {
            eprintln!("napkin: reading the clipboard failed: {error}");
            return Ok(ClipboardContent::Empty);
        }
    };
    let Some(mime) = choose(&offered) else {
        return Ok(ClipboardContent::Empty);
    };
    if mime == SVG_TYPE {
        return Ok(ClipboardContent::Svg);
    }
    let mut bytes = Vec::new();
    let read = paste::get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        MimeType::Specific(mime),
    )
    .map_err(|error| error.to_string())
    .and_then(|(reader, _)| {
        reader
            .take(MAX_READ_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())
    });
    if let Err(error) = read {
        eprintln!("napkin: reading {mime} from the clipboard failed: {error}");
        return Ok(ClipboardContent::Empty);
    }
    Ok(if IMAGE_TYPES.contains(&mime) {
        ClipboardContent::Image(bytes)
    } else {
        ClipboardContent::Text(String::from_utf8_lossy(&bytes).into_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_win_over_text_and_svg_is_reported() {
        let offered = |types: &[&str]| types.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        assert_eq!(
            choose(&offered(&["text/plain", "image/jpeg", "image/png"])),
            Some("image/png")
        );
        assert_eq!(
            choose(&offered(&["image/webp", "text/html"])),
            Some("image/webp")
        );
        assert_eq!(
            choose(&offered(&["image/svg+xml", "text/plain"])),
            Some("image/svg+xml")
        );
        assert_eq!(
            choose(&offered(&["image/svg+xml", "image/gif"])),
            Some("image/gif")
        );
        assert_eq!(
            choose(&offered(&["text/plain;charset=utf-8"])),
            Some("text/plain;charset=utf-8")
        );
        assert_eq!(choose(&offered(&[])), None);
    }
}
