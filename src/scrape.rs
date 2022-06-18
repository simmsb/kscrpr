use color_eyre::{eyre::eyre, Result};
use indicatif::ProgressBar;
use itertools::Itertools;
use once_cell::sync::OnceCell;
use reqwest::{Client, Url};
use scraper::{Html, Selector};

use crate::archive::{Archive, Tag};
use crate::client::client;
use crate::filesystem::FileSystem;
use crate::opts::opts;
use crate::utils::fuck_error;

pub async fn by_id(id: u32) -> Result<(Archive, DownloadSize)> {
    let config = opts();
    let client = client();

    let url = config.base_url.join("archive/")?.join(&id.to_string())?;

    fetch_archive(client, &url).await
}

fn tag_view_archive_selector() -> &'static Selector {
    static ARCHIVE_SELECTOR: OnceCell<Selector> = OnceCell::new();
    ARCHIVE_SELECTOR.get_or_init(|| {
        Selector::parse("html body main section#archives.feed div.entries article.entry a").unwrap()
    })
}

fn article_download_selector() -> &'static Selector {
    static ARCHIVE_SELECTOR: OnceCell<Selector> = OnceCell::new();
    ARCHIVE_SELECTOR.get_or_init(|| Selector::parse(".download").unwrap())
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct DownloadSize(pub u32);

#[derive(serde::Deserialize)]
struct SluggedMeta {
    // id: u32,
    slug: String,
    name: String,
}

#[derive(serde::Deserialize)]
pub struct ArchiveMeta {
    id: u32,
    title: String,
    pages: u16,
    size: DownloadSize,
    artists: Vec<SluggedMeta>,
    #[serde(default)]
    parodies: Vec<SluggedMeta>,
    #[serde(default)]
    tags: Vec<SluggedMeta>,
}

impl ArchiveMeta {
    pub fn as_archive(&self, base_url: Url, download_url: Url) -> Archive {
        Archive {
            id: self.id,
            name: self.title.clone(),
            artist: self
                .artists
                .first()
                .map(|a| a.name.clone())
                .ok_or_else(|| eyre!("Archive had no artist?"))
                .unwrap(),
            parody: self
                .parodies
                .first()
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "original".to_owned()),
            tags: self
                .tags
                .iter()
                .map(|t| Tag {
                    path: t.slug.clone(),
                    name: t.name.clone(),
                })
                .collect_vec(),
            num_pages: self.pages,
            base_url,
            download_url,
        }
    }
}

async fn fetch_archive(client: &Client, url: &Url) -> Result<(Archive, DownloadSize)> {
    tracing::debug!(%url, "Fetching archive");

    let meta: ArchiveMeta = client.get(url.join(".json")?).send().await?.json().await?;

    let page = client.get(url.as_str()).send().await?.text().await?;
    let doc = Html::parse_document(&page);

    let download_url = doc
        .select(article_download_selector())
        .next()
        .unwrap()
        .value()
        .attr("href")
        .unwrap();

    let download_url = Url::parse(download_url)?;

    Ok((meta.as_archive(url.clone(), download_url), meta.size))
}

pub async fn fetch_tag_page(
    fs: &FileSystem,
    tag: &str,
    page_n: u32,
    msg_bar: &ProgressBar,
    prog_bar: &ProgressBar,
) -> Result<Option<Vec<(Archive, DownloadSize)>>> {
    tracing::debug!(tag, page_n, "Fetching tag page");
    msg_bar.set_prefix("Fetching page");
    msg_bar.set_message("");

    let config = opts();
    let client = client();

    let mut url = config.base_url.clone();
    url.path_segments_mut().unwrap().push("tags").push(tag);
    url.query_pairs_mut()
        .append_pair("page", &format!("{}", page_n));

    let page = client.get(url).send().await?.text().await?;

    if page.contains("Not yet available") {
        tracing::info!(tag, "Reached last tags page at {}", page_n);
        return Ok(None);
    }

    let doc = Html::parse_document(&page);

    let mut archives = vec![];

    msg_bar.set_prefix("Fetching metadata");

    let urls = doc.select(tag_view_archive_selector()).collect::<Vec<_>>();
    prog_bar.set_length(urls.len() as u64);
    prog_bar.set_position(0);
    for article_url in urls {
        let url = match article_url.value().attr("href") {
            Some(u) => u,
            None => {
                tracing::debug!(element = ?article_url, "Article link was missing a url");
                continue;
            }
        };

        msg_bar.set_message(url.to_owned());

        let url = config.base_url.join(url)?;

        let id: u32 = url
            .path_segments()
            .unwrap()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();

        if fs.has_archive(id) {
            tracing::debug!(%id, "Not fetching archive as it already exists");
            continue;
        }

        match fetch_archive(client, &url).await {
            Ok(a) => archives.push(a),
            Err(e) => {
                tracing::error!(error = fuck_error(&e), %url, "Failed to fetch archive");
            }
        }

        prog_bar.inc(1);
    }

    Ok(Some(archives))
}
