use std::io::{Cursor, Read, Seek};

use bytes::Bytes;
use color_eyre::Result;
use url::Url;
use zip::ZipArchive;

use crate::client::client;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Tag {
    pub path: String,
    pub name: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Archive {
    pub id: u32,
    pub name: String,
    pub artist: String,
    pub parody: String,
    pub tags: Vec<Tag>,
    pub num_pages: u16,
    pub base_url: Url,
    pub download_url: Url,
}

impl Archive {
    pub async fn download(&self, inspector: impl Fn(Option<u64>, &Bytes)) -> Result<ZipArchive<impl Read + Seek>> {
        let mut body = client()
            .get(self.download_url.as_str())
            .send()
            .await?;

        let mut v = Vec::new();

        if let Some(size_hint) = body.content_length() {
            v.reserve(size_hint as usize);
        }

        while let Some(buf) = body.chunk().await? {
            v.extend_from_slice(&buf);
            inspector(body.content_length(), &buf);
        }

        let zip = zip::ZipArchive::new(Cursor::new(v))?;

        Ok(zip)
    }

    pub fn pretty_single_line(&self) -> String {
        format!("[{}] {}", self.artist, self.name)
    }
}
