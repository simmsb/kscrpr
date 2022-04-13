use std::path::PathBuf;

use color_eyre::SectionExt;
use color_eyre::{eyre::eyre, Help, Result};
use tracing::{debug, info_span, Instrument};

use crate::archives::Archive;
use crate::config::{self, Config};
use crate::searcher::Searcher;
use crate::utils::fuck_error;

pub struct FileSystem {
    config: &'static Config,
    pub sled_db: sled::Db,
    pub searcher: Searcher,
}

impl FileSystem {
    pub fn open() -> Result<Self> {
        let config = config::config();

        tracing::debug!("Ensuring data directory at {:?}", config.base_dir);

        let dirs = [
            "data/by_ids/",
            "data/by_tags/",
            "data/by_artist/",
            "meta/tantivy/",
            "meta/sled/",
        ];

        for dir in dirs {
            std::fs::create_dir_all(config.base_dir.join(dir))?;
        }

        let sled_db = sled::open(config.base_dir.join("meta/sled/"))
            .note("While opening/creating the sled database")?;

        let searcher = Searcher::new(&config.base_dir.join("meta/tantivy/"))
            .note("While opening/creating the tantivy database")?;

        Ok(Self {
            config,
            sled_db,
            searcher,
        })
    }

    pub fn reset_tantivy_dir() {
        let config = config::config();
        let _ = std::fs::remove_dir_all(config.base_dir.join("meta/tantivy/"));
    }

    pub fn reset_tags_dir() {
        let config = config::config();
        let _ = std::fs::remove_dir_all(config.base_dir.join("data/by_tags/"));
    }

    pub fn reset_artists_dir() {
        let config = config::config();
        let _ = std::fs::remove_dir_all(config.base_dir.join("data/by_artist/"));
    }

    pub fn data_dir(&self) -> PathBuf {
        self.config.base_dir.join("data/")
    }

    pub fn meta_dir(&self) -> PathBuf {
        self.config.base_dir.join("meta/")
    }

    pub fn tantivy_dir(&self) -> PathBuf {
        self.meta_dir().join("sled/")
    }

    pub fn sled_dir(&self) -> PathBuf {
        self.meta_dir().join("sled/")
    }

    pub fn id_dir(&self) -> PathBuf {
        self.data_dir().join("by_ids/")
    }

    pub fn tag_dir(&self) -> PathBuf {
        self.data_dir().join("by_tags/")
    }

    pub fn artist_dir(&self) -> PathBuf {
        self.data_dir().join("by_artist/")
    }

    pub fn dir_of_id(&self, id: u32) -> PathBuf {
        self.id_dir().join(format!("{id}/"))
    }

    pub fn dir_of_tag(&self, tag: &str) -> PathBuf {
        self.tag_dir().join(format!("{tag}/"))
    }

    pub fn dir_of_artist(&self, artist: &str) -> PathBuf {
        self.artist_dir().join(format!("{artist}/"))
    }

    pub fn has_archive(&self, id: u32) -> bool {
        return self.dir_of_id(id).exists();
    }

    pub fn build_symlinks_for(&self, archive: &Archive) -> Result<()> {
        let target_dir = self.dir_of_id(archive.id);
        for tag in &archive.tags {
            let tag_dir = self.dir_of_tag(&tag.name);
            std::fs::create_dir_all(&tag_dir)?;
            let src_dir_v = target_dir.clone().to_string_lossy().to_string();
            let dst_dir_v = tag_dir.join(&archive.name).to_string_lossy().to_string();
            symlink::symlink_dir(&target_dir, tag_dir.join(&archive.name))
                .note("While symlinking the tag directory")
                .with_section(move || src_dir_v.header("Source:"))
                .with_section(move || dst_dir_v.header("Destination:"))?;
        }

        let artist_dir = self.dir_of_artist(&archive.artist);
        std::fs::create_dir_all(&artist_dir)?;
        symlink::symlink_dir(&target_dir, artist_dir.join(&archive.name))?;

        Ok(())
    }

    pub async fn add_archive(&self, archive: &Archive, force: bool) -> Result<bool> {
        if !force && self.has_archive(archive.id) {
            debug!(id = %archive.id, name = %archive.name, "Not downloading archive as it already exists");
            return Ok(false);
        }

        let mut zip = archive
            .download()
            .instrument(
                info_span!("Downloading archive zip", id = archive.id, name = %archive.name),
            )
            .await?;

        let target_dir = self.dir_of_id(archive.id);
        std::fs::create_dir_all(&target_dir)?;

        if let Err(e) = zip.extract(&target_dir) {
            tracing::error!(
                error = fuck_error(&e.into()),
                id = archive.id,
                name = %archive.name,
                "Failed to extract zip, trating this as a non-fatal error though"
            );
            return Ok(false);
        }

        if let Err(e) = self.build_symlinks_for(archive) {
            tracing::error!(
                error = fuck_error(&e),
                id = archive.id,
                name = %archive.name,
                "Failed to create symlinks?, trating this as a non-fatal error though"
            );
        }

        self.sled_db
            .insert(archive.id.to_be_bytes(), serde_cbor::to_vec(&archive)?)?;
        self.searcher.add_archive(archive).await?;

        Ok(true)
    }

    pub async fn with_all_tags(&self, tags: &[String]) -> Result<Vec<Archive>> {
        let doc_ids = self
            .searcher
            .with_all_tags(tags)
            .instrument(tracing::debug_span!(
                "Searching for archives with all given tags",
                ?tags
            ))
            .await?;

        self.fetch_inner(doc_ids)
    }

    pub async fn search(
        &self,
        query: &str,
        default_indexes: &[&str],
        max: Option<usize>,
    ) -> Result<Vec<Archive>> {
        let doc_ids = self
            .searcher
            .search(query, default_indexes, max)
            .instrument(tracing::debug_span!(
                "Searching for archives matching the given query",
                ?query,
                ?default_indexes,
                ?max
            ))
            .await?;

        self.fetch_inner(doc_ids)
    }

    fn fetch_inner(&self, doc_ids: Vec<u32>) -> Result<Vec<Archive>> {
        let r = doc_ids
            .into_iter()
            .filter_map(|id| match self.fetch_doc(id) {
                Ok(a) => Some(a),
                Err(e) => {
                    tracing::error!(reason = fuck_error(&e), id, "Failed to fetch document");
                    None
                }
            })
            .collect::<Vec<_>>();
        Ok(r)
    }

    pub fn fetch_doc(&self, id: u32) -> Result<Archive> {
        let v = self
            .sled_db
            .get(id.to_be_bytes())?
            .ok_or_else(|| eyre!("Document {} does not exist", id))?;

        let a = serde_cbor::from_slice::<Archive>(&v)?;
        Ok(a)
    }
}
