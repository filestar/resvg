// Copyright 2018 the Resvg Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

mod clippath;
mod converter;
mod filter;
mod image;
mod marker;
mod mask;
mod options;
mod paint_server;
mod shapes;
mod style;
mod svgtree;
mod switch;
mod units;
mod use_node;

#[cfg(feature = "text")]
mod text;
#[cfg(feature = "text")]
pub(crate) use converter::Cache;
pub use image::{ImageHrefDataResolverFn, ImageHrefResolver, ImageHrefStringResolverFn};
pub use options::Options;
#[cfg(feature = "writer")]
pub(crate) use svgtree::{AId, EId};

/// List of all errors.
#[derive(Debug)]
pub enum Error {
    /// Only UTF-8 and UTF-16 content are supported.
    NotAnUtf8Str,

    /// `svgz` feature is required to parse SVGZ data.
    SvgzFeatureNotEnabled,

    /// Compressed SVG must use the GZip algorithm.
    MalformedGZip,

    /// We do not allow SVG with more than 1_000_000 elements for security reasons.
    ElementsLimitReached,

    /// SVG doesn't have a valid size.
    ///
    /// Occurs when width and/or height are <= 0.
    ///
    /// Also occurs if width, height and viewBox are not set.
    InvalidSize,

    /// Failed to parse an SVG data.
    ParsingFailed(roxmltree::Error),
}

impl From<roxmltree::Error> for Error {
    fn from(e: roxmltree::Error) -> Self {
        Error::ParsingFailed(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            Error::NotAnUtf8Str => {
                write!(f, "provided data has neither a UTF-8 nor a UTF-16 encoding")
            }
            Self::SvgzFeatureNotEnabled => {
                write!(f, "enable svgz cargo feature to decode SVGZ data")
            }
            Error::MalformedGZip => {
                write!(f, "provided data has a malformed GZip content")
            }
            Error::ElementsLimitReached => {
                write!(f, "the maximum number of SVG elements has been reached")
            }
            Error::InvalidSize => {
                write!(f, "SVG has an invalid size")
            }
            Error::ParsingFailed(ref e) => {
                write!(f, "SVG data parsing failed cause {}", e)
            }
        }
    }
}

impl std::error::Error for Error {}

pub(crate) trait OptionLog {
    fn log_none<F: FnOnce()>(self, f: F) -> Self;
}

impl<T> OptionLog for Option<T> {
    #[inline]
    fn log_none<F: FnOnce()>(self, f: F) -> Self {
        self.or_else(|| {
            f();
            None
        })
    }
}

impl crate::Tree {
    /// Parses `Tree` from an SVG data.
    ///
    /// Can contain an SVG string or a gzip compressed data.
    pub fn from_data(data: &[u8], opt: &Options) -> Result<Self, Error> {
        if data.starts_with(&[0x1f, 0x8b]) {
            #[cfg(feature = "svgz")]
            {
                let data = decompress_svgz(data)?;
                let text = decode_text(&data)?;
                Self::from_str(&text, opt)
            }

            #[cfg(not(feature = "svgz"))]
            Err(Error::SvgzFeatureNotEnabled)
        } else {
            let text = decode_text(data)?;
            Self::from_str(&text, opt)
        }
    }

    /// Similar to the `from_data` method, except that it ignores all `image` elements linking to
    /// external files, as required by the SVG specification when SVG files are loaded
    /// for `<image href="..." />` tags.
    pub fn from_data_nested(data: &[u8], opt: &Options) -> Result<Self, Error> {
        let nested_opt = Options {
            resources_dir: None,
            dpi: opt.dpi,
            font_size: opt.font_size,
            languages: opt.languages.clone(),
            shape_rendering: opt.shape_rendering,
            text_rendering: opt.text_rendering,
            image_rendering: opt.image_rendering,
            default_size: opt.default_size,
            image_href_resolver: ImageHrefResolver {
                resolve_data: Box::new(|a, b, c| (opt.image_href_resolver.resolve_data)(a, b, c)),
                // External images should be ignored.
                resolve_string: Box::new(|_, _| None),
            },
            // In the referenced SVG, we start with the unmodified user-provided
            // fontdb, not the one from the cache.
            #[cfg(feature = "text")]
            fontdb: opt.fontdb.clone(),
            // Can't clone the resolver, so we create a new one that forwards to it.
            #[cfg(feature = "text")]
            font_resolver: crate::FontResolver {
                select_font: Box::new(|font, db| (opt.font_resolver.select_font)(font, db)),
                select_fallback: Box::new(|c, used_fonts, db| {
                    (opt.font_resolver.select_fallback)(c, used_fonts, db)
                }),
            },
            ..Options::default()
        };

        Self::from_data(data, &nested_opt)
    }

    /// Parses `Tree` from an SVG string.
    pub fn from_str(text: &str, opt: &Options) -> Result<Self, Error> {
        let text = preprocess_text(text, opt);

        let xml_opt = roxmltree::ParsingOptions {
            allow_dtd: true,
            forgiving: opt.forgiving,
            ..Default::default()
        };

        let doc = roxmltree::Document::parse_with_options(&text, xml_opt)
            .map_err(Error::ParsingFailed)?;
        for e in doc.forgiven_errors() {
            log::warn!("Ignored an attribute that could not be read: {}.", e);
        }

        Self::from_xmltree(&doc, opt)
    }

    /// Parses `Tree` from `roxmltree::Document`.
    pub fn from_xmltree(doc: &roxmltree::Document, opt: &Options) -> Result<Self, Error> {
        let doc = svgtree::Document::parse_tree(doc, opt.style_sheet.as_deref())?;
        self::converter::convert_doc(&doc, opt)
    }
}

/// Removes characters XML does not allow before parsing (Filestar).
///
/// Only NUL for now, and only when `Options::forgiving` is set.
pub fn preprocess_text<'t>(text: &'t str, opt: &Options) -> std::borrow::Cow<'t, str> {
    if opt.forgiving && text.contains('\0') {
        log::warn!("Removed one or more NUL characters from the input.");
        std::borrow::Cow::Owned(text.replace('\0', ""))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// Decodes SVG data as UTF-8, or as UTF-16 (Filestar).
///
/// UTF-16 is recognised by its byte order mark. Without one, UTF-16 is tried
/// (little endian first) only when the data is not valid UTF-8.
pub fn decode_text(data: &[u8]) -> Result<std::borrow::Cow<'_, str>, Error> {
    if let Ok(text) = std::str::from_utf8(data) {
        return Ok(std::borrow::Cow::Borrowed(text.strip_prefix('\u{feff}').unwrap_or(text)));
    }

    string_from_utf16_bytes(data)
        .map(std::borrow::Cow::Owned)
        .map_err(|_| Error::NotAnUtf8Str)
}

fn string_from_utf16_bytes(bytes: &[u8]) -> Result<String, ()> {
    fn decode(bytes: &[u8], to_u16: fn([u8; 2]) -> u16) -> Result<String, ()> {
        if bytes.len() % 2 != 0 {
            return Err(());
        }
        let values: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| to_u16([c[0], c[1]]))
            .collect();
        String::from_utf16(&values).map_err(|_| ())
    }

    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        decode(rest, u16::from_le_bytes)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        decode(rest, u16::from_be_bytes)
    } else {
        decode(bytes, u16::from_le_bytes).or_else(|_| decode(bytes, u16::from_be_bytes))
    }
}

/// Decompresses an SVGZ file.
#[cfg(feature = "svgz")]
pub fn decompress_svgz(data: &[u8]) -> Result<Vec<u8>, Error> {
    use std::io::Read;

    let mut decoder = flate2::read::GzDecoder::new(data);
    let mut decoded = Vec::with_capacity(data.len() * 2);
    decoder
        .read_to_end(&mut decoded)
        .map_err(|_| Error::MalformedGZip)?;
    Ok(decoded)
}

#[inline]
pub(crate) fn f32_bound(min: f32, val: f32, max: f32) -> f32 {
    debug_assert!(min.is_finite());
    debug_assert!(max.is_finite());

    if val > max {
        max
    } else if val >= min {
        val
    } else {
        // Catches `val < min` as well as a NaN `val`.
        min
    }
}
