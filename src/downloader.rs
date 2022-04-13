use color_eyre::Result;
use once_cell::sync::OnceCell;
use reqwest::{Client, Url};
use scraper::{Html, Selector};

use crate::archives::{Archive, Tag};
use crate::client::client;
use crate::config::config;
use crate::filesystem::FileSystem;
use crate::utils::fuck_error;

pub async fn by_id(id: u32) -> Result<Archive> {
    let config = config();
    let client = client();

    let url = config.base_url.join(&id.to_string())?;

    fetch_archive(client, &url).await
}

fn tag_view_archive_selector() -> &'static Selector {
    static ARCHIVE_SELECTOR: OnceCell<Selector> = OnceCell::new();
    ARCHIVE_SELECTOR.get_or_init(|| {
        Selector::parse("html body main section#archives.feed div.entries article.entry a").unwrap()
    })
}

fn article_name_selector() -> &'static Selector {
    static ARCHIVE_SELECTOR: OnceCell<Selector> = OnceCell::new();
    ARCHIVE_SELECTOR
        .get_or_init(|| Selector::parse("html body main#archive div.metadata h1.title").unwrap())
}

fn article_artist_selector() -> &'static Selector {
    static ARCHIVE_SELECTOR: OnceCell<Selector> = OnceCell::new();
    ARCHIVE_SELECTOR
        .get_or_init(|| Selector::parse(".artists > td:nth-child(2) > a:nth-child(1)").unwrap())
}

fn article_parody_selector() -> &'static Selector {
    static ARCHIVE_SELECTOR: OnceCell<Selector> = OnceCell::new();
    ARCHIVE_SELECTOR
        .get_or_init(|| Selector::parse(".parodies > td:nth-child(2) > a:nth-child(1)").unwrap())
}

fn article_tags_selector() -> &'static Selector {
    static ARCHIVE_SELECTOR: OnceCell<Selector> = OnceCell::new();
    ARCHIVE_SELECTOR.get_or_init(|| Selector::parse(".tags > td:nth-child(2) > a").unwrap())
}

fn article_pages_selector() -> &'static Selector {
    static ARCHIVE_SELECTOR: OnceCell<Selector> = OnceCell::new();
    ARCHIVE_SELECTOR.get_or_init(|| Selector::parse(".pages > td:nth-child(2)").unwrap())
}

fn article_download_selector() -> &'static Selector {
    static ARCHIVE_SELECTOR: OnceCell<Selector> = OnceCell::new();
    ARCHIVE_SELECTOR.get_or_init(|| Selector::parse(".download").unwrap())
}

async fn fetch_archive(client: &Client, url: &Url) -> Result<Archive> {
    tracing::debug!(%url, "Fetching archive");

    let page = client.get(url.as_str()).send().await?.text().await?;
    let doc = Html::parse_document(&page);

    let id: u32 = url
        .path_segments()
        .unwrap()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();

    let name = doc
        .select(article_name_selector())
        .next()
        .unwrap()
        .text()
        .next()
        .unwrap()
        .to_owned();

    let artist = doc
        .select(article_artist_selector())
        .next()
        .unwrap()
        .text()
        .next()
        .unwrap()
        .to_owned();

    let parody = doc
        .select(article_parody_selector())
        .next()
        .unwrap()
        .text()
        .next()
        .unwrap()
        .to_owned();

    let tags = doc
        .select(article_tags_selector())
        .map(|e| {
            let path = e.value().attr("href").unwrap().to_owned();
            let name = e.text().next().unwrap().to_owned();

            Tag { path, name }
        })
        .collect::<Vec<_>>();

    let num_pages: u16 = doc
        .select(article_pages_selector())
        .next()
        .unwrap()
        .text()
        .next()
        .unwrap()
        .parse()
        .unwrap();

    let download_url = doc
        .select(article_download_selector())
        .next()
        .unwrap()
        .value()
        .attr("href")
        .unwrap();

    let download_url = Url::parse(download_url).unwrap();

    Ok(Archive {
        id,
        name,
        artist,
        parody,
        tags,
        num_pages,
        base_url: url.clone(),
        download_url,
    })
}

pub async fn fetch_tag_page(fs: &FileSystem, tag: &str, page_n: u32) -> Result<Option<Vec<Archive>>> {
    tracing::debug!(tag, page_n, "Fetching tag page");

    let config = config();
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

    for article_url in doc.select(tag_view_archive_selector()) {
        let url = match article_url.value().attr("href") {
            Some(u) => u,
            None => {
                tracing::debug!(element = ?article_url, "Article link was missing a url");
                continue;
            }
        };

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
    }

    Ok(Some(archives))
}
