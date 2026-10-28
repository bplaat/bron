/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The freedesktop thumbnailer: `imageview --thumbnail -s SIZE INPUT OUTPUT`.

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use imageview_core::{decode_media, render_thumbnail};

const USAGE: &str = "Usage: imageview --thumbnail -s SIZE INPUT OUTPUT";

struct Arguments {
    size: u32,
    input: PathBuf,
    output: PathBuf,
}

/// Writes a PNG thumbnail for the freedesktop thumbnailer arguments `-s SIZE INPUT OUTPUT`,
/// returning the process exit code.
pub(crate) fn thumbnail(arguments: Vec<OsString>) -> i32 {
    match parse_arguments(arguments).and_then(run) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

fn parse_arguments(arguments: Vec<OsString>) -> Result<Arguments, String> {
    let mut size = None;
    let mut files = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "-s" {
            let value = arguments.next().ok_or(USAGE)?;
            let value = value.to_string_lossy();
            size = Some(
                value
                    .parse()
                    .map_err(|_| format!("Invalid size: {value}"))?,
            );
        } else {
            files.push(argument);
        }
    }
    let (Some(size), [input, output]) = (size, files.as_slice()) else {
        return Err(USAGE.into());
    };
    Ok(Arguments {
        size,
        input: input_path(input)?,
        output: PathBuf::from(output),
    })
}

// File managers pass a path or a percent-encoded file URI.
fn input_path(input: &OsString) -> Result<PathBuf, String> {
    let Some(mut rest) = input.as_bytes().strip_prefix(b"file://") else {
        return Ok(PathBuf::from(input));
    };
    let mut bytes = Vec::with_capacity(rest.len());
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = tail
                .get(..2)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                .ok_or("Invalid file URI")?;
            bytes.push(hex);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

fn run(arguments: Arguments) -> Result<(), String> {
    let bytes = std::fs::read(&arguments.input).map_err(|error| error.to_string())?;
    let media = decode_media(&bytes)?;
    let (width, height) = media.size();
    let bitmap = render_thumbnail(media, arguments.size).ok_or("Could not render the thumbnail")?;
    let (width, height) = (
        (width.round() as u64).to_string(),
        (height.round() as u64).to_string(),
    );
    // The file manager adds the URI and modification time when it stores the thumbnail.
    let options = image::EncodeOptions {
        text: &[
            ("Thumb::Image::Width", &width),
            ("Thumb::Image::Height", &height),
            ("Software", "ImageView"),
        ],
        ..image::EncodeOptions::default()
    };
    let png = image::encode_with_options(&bitmap, image::Format::Png, options)
        .map_err(|error| error.to_string())?;
    std::fs::write(&arguments.output, png).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Arguments, String> {
        parse_arguments(arguments.iter().map(OsString::from).collect())
    }

    #[test]
    fn arguments_follow_the_thumbnailer_specification() {
        let arguments = parse(&["-s", "256", "in.png", "out.png"]).expect("valid arguments");
        assert_eq!(arguments.size, 256);
        assert_eq!(arguments.input, PathBuf::from("in.png"));
        assert_eq!(arguments.output, PathBuf::from("out.png"));
        assert!(parse(&["in.png", "out.png"]).is_err());
        assert!(parse(&["-s", "big", "in.png", "out.png"]).is_err());
    }

    #[test]
    fn file_uris_become_paths() {
        let path = |input: &str| input_path(&OsString::from(input)).expect("valid input");
        assert_eq!(
            path("file:///home/me/My%20Photos/a.qoi"),
            PathBuf::from("/home/me/My Photos/a.qoi")
        );
        assert_eq!(path("/tmp/a.png"), PathBuf::from("/tmp/a.png"));
        assert!(input_path(&OsString::from("file:///a%2")).is_err());
    }

    #[test]
    fn images_render_into_png_thumbnails() {
        let output =
            std::env::temp_dir().join(format!("imageview-thumbnail-{}.png", std::process::id()));
        run(Arguments {
            size: 64,
            input: PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/tiger.svg")),
            output: output.clone(),
        })
        .expect("thumbnail");
        let png = std::fs::read(&output).expect("written thumbnail");
        std::fs::remove_file(output).expect("remove thumbnail");
        let thumbnail = image::decode(&png).expect("valid PNG");
        assert_eq!((thumbnail.width(), thumbnail.height()), (64, 64));
        let text = |key: &str| {
            thumbnail
                .text()
                .iter()
                .find(|(keyword, _)| keyword == key)
                .map(|(_, text)| text.as_str())
        };
        assert_eq!(text("Thumb::Image::Width"), Some("600"));
        assert_eq!(text("Thumb::Image::Height"), Some("600"));
    }
}
