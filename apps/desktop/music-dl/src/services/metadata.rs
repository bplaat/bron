/*
 * Copyright (c) 2024-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use anyhow::Result;
use percent_encoding::utf8_percent_encode;
use small_http::{Client, Request};

use crate::structs::deezer::{
    Album, AlbumList, AlbumSmall, ArtistList, ArtistSmall, SearchTrackList, Track,
};

const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:135.0) Gecko/20100101 Firefox/135.0";

#[derive(Clone)]
pub(crate) struct MetadataService {
    client: Client,
}

impl MetadataService {
    pub(crate) fn new() -> Self {
        Self {
            client: Client::new().header("User-Agent", USER_AGENT),
        }
    }

    pub(crate) fn search(&mut self, query: &str) -> Result<SearchTrackList> {
        Ok(self
            .client
            .fetch(Request::get(format!(
                "https://api.deezer.com/search?q={}",
                utf8_percent_encode(query, percent_encoding::NON_ALPHANUMERIC)
            )))?
            .into_json::<SearchTrackList>()?)
    }

    pub(crate) fn search_album(&mut self, query: &str) -> Result<Vec<AlbumSmall>> {
        Ok(self
            .client
            .fetch(Request::get(format!(
                "https://api.deezer.com/search/album?q={}",
                utf8_percent_encode(query, percent_encoding::NON_ALPHANUMERIC)
            )))?
            .into_json::<AlbumList>()?
            .data)
    }

    pub(crate) fn seach_artist(&mut self, query: &str) -> Result<Vec<ArtistSmall>> {
        Ok(self
            .client
            .fetch(Request::get(format!(
                "https://api.deezer.com/search/artist?q={}",
                utf8_percent_encode(query, percent_encoding::NON_ALPHANUMERIC)
            )))?
            .into_json::<ArtistList>()?
            .data)
    }

    pub(crate) fn get_artist_albums(&mut self, artist_id: i64) -> Result<Vec<AlbumSmall>> {
        Ok(self
            .client
            .fetch(Request::get(format!(
                "https://api.deezer.com/artist/{artist_id}/albums?limit=500",
            )))?
            .into_json::<AlbumList>()?
            .data)
    }

    pub(crate) fn get_album(&mut self, album_id: i64) -> Result<Album> {
        Ok(self
            .client
            .fetch(Request::get(format!(
                "https://api.deezer.com/album/{album_id}?limit=500",
            )))?
            .into_json::<Album>()?)
    }

    pub(crate) fn get_track(&mut self, track_id: i64) -> Result<Track> {
        Ok(self
            .client
            .fetch(Request::get(format!(
                "https://api.deezer.com/track/{track_id}",
            )))?
            .into_json::<Track>()?)
    }

    pub(crate) fn download(&mut self, cover_url: &str) -> Result<Vec<u8>> {
        Ok(self
            .client
            .fetch(Request::get(cover_url))?
            .body
            .into_owned())
    }
}
