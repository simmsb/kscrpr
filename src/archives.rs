use std::io::{Cursor, Read, Seek};

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
    pub async fn download(&self) -> Result<ZipArchive<impl Read + Seek>> {
        let body = client()
            .get(self.download_url.as_str())
            .send()
            .await?
            .bytes()
            .await?;

        let zip = zip::ZipArchive::new(Cursor::new(body))?;

        Ok(zip)
    }
}
