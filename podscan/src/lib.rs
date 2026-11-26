//! podscan: scans the podcast library and builds `catalog.json` and M3U8 playlists.
//!
//! The catalog model ([`catalog`]) is public: the page generator uses it.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod filename;
pub mod m3u;
pub mod programs;
pub mod scan;
pub mod text;
