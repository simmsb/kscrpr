use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use color_eyre::SectionExt;
use color_eyre::{eyre::eyre, Help, Result};
use indicatif::ProgressBar;
use printpdf::{image_crate::GenericImageView, PdfDocument, Px};
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
            "rendered/by_ids/",
            "rendered/by_tags/",
            "rendered/by_artist/",
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

    pub fn reset_rendered_dir() {
        let config = config::config();
        let _ = std::fs::remove_dir_all(config.base_dir.join("rendered/by_tags/"));
        let _ = std::fs::remove_dir_all(config.base_dir.join("rendered/by_artist/"));
    }

    pub fn data_dir(&self) -> PathBuf {
        self.config.base_dir.join("data/")
    }

    pub fn meta_dir(&self) -> PathBuf {
        self.config.base_dir.join("meta/")
    }

    pub fn rendered_dir(&self) -> PathBuf {
        self.config.base_dir.join("rendered/")
    }

    pub fn tantivy_dir(&self) -> PathBuf {
        self.meta_dir().join("sled/")
    }

    pub fn sled_dir(&self) -> PathBuf {
        self.meta_dir().join("sled/")
    }

    pub fn data_id_dir(&self) -> PathBuf {
        self.data_dir().join("by_ids/")
    }

    pub fn data_tag_dir(&self) -> PathBuf {
        self.data_dir().join("by_tags/")
    }

    pub fn data_artist_dir(&self) -> PathBuf {
        self.data_dir().join("by_artist/")
    }

    pub fn data_dir_of_id(&self, id: u32) -> PathBuf {
        self.data_id_dir().join(format!("{id}/"))
    }

    pub fn data_dir_of_tag(&self, tag: &str) -> PathBuf {
        self.data_tag_dir().join(format!("{tag}/"))
    }

    pub fn data_dir_of_artist(&self, artist: &str) -> PathBuf {
        self.data_artist_dir().join(format!("{artist}/"))
    }

    pub fn data_dir_for_archive_by_artist(&self, archive: &Archive) -> PathBuf {
        let name = format!("{}-{}", archive.name, archive.id);
        self.data_dir_of_artist(&archive.artist).join(name)
    }

    pub fn data_dir_for_archive_by_tag(&self, tag: &str, archive: &Archive) -> PathBuf {
        let name = format!("{}-{}", archive.name, archive.id);
        self.data_dir_of_tag(tag).join(name)
    }

    pub fn rendered_id_dir(&self) -> PathBuf {
        self.rendered_dir().join("by_ids/")
    }

    pub fn rendered_tag_dir(&self) -> PathBuf {
        self.rendered_dir().join("by_tags/")
    }

    pub fn rendered_artist_dir(&self) -> PathBuf {
        self.rendered_dir().join("by_artist/")
    }

    pub fn rendered_file_of_id(&self, id: u32) -> PathBuf {
        self.rendered_id_dir().join(format!("{id}.pdf"))
    }

    pub fn rendered_dir_of_tag(&self, tag: &str) -> PathBuf {
        self.rendered_tag_dir().join(format!("{tag}/"))
    }

    pub fn rendered_dir_of_artist(&self, artist: &str) -> PathBuf {
        self.rendered_artist_dir().join(format!("{artist}/"))
    }

    pub fn rendered_file_for_archive_by_artist(&self, archive: &Archive) -> PathBuf {
        let name = format!("{}-{}.pdf", archive.name, archive.id);
        self.rendered_dir_of_artist(&archive.artist).join(name)
    }

    pub fn rendered_file_for_archive_by_tag(&self, tag: &str, archive: &Archive) -> PathBuf {
        let name = format!("{}-{}.pdf", archive.name, archive.id);
        self.rendered_dir_of_tag(tag).join(name)
    }

    pub fn has_archive(&self, id: u32) -> bool {
        return self.data_dir_of_id(id).exists();
    }

    pub fn build_data_symlinks_for(&self, archive: &Archive) -> Result<()> {
        let target_dir = self.data_dir_of_id(archive.id);
        for tag in &archive.tags {
            let tag_dir = self.data_dir_for_archive_by_tag(&tag.name, &archive);
            std::fs::create_dir_all(tag_dir.parent().unwrap())?;

            let src_dir_v = target_dir.clone().to_string_lossy().to_string();
            let dst_dir_v = tag_dir.to_string_lossy().to_string();

            symlink::symlink_dir(&target_dir, tag_dir)
                .note("While symlinking the tag directory")
                .with_section(move || src_dir_v.header("Source:"))
                .with_section(move || dst_dir_v.header("Destination:"))?;
        }

        let artist_dir = self.data_dir_for_archive_by_artist(&archive);
        std::fs::create_dir_all(artist_dir.parent().unwrap())?;
        symlink::symlink_dir(&target_dir, artist_dir)?;

        Ok(())
    }

    pub async fn add_archive(
        &self,
        archive: &Archive,
        force: bool,
        msg_bar: &ProgressBar,
        prog_bar: &ProgressBar,
    ) -> Result<bool> {
        if !force && self.has_archive(archive.id) {
            debug!(id = %archive.id, name = %archive.name, "Not downloading archive as it already exists");
            return Ok(false);
        }

        msg_bar.set_prefix("Downloading zip");
        msg_bar.set_message(format!("({})[{}]", archive.id, archive.name));
        prog_bar.set_length(0);
        prog_bar.set_position(0);

        let mut zip = archive
            .download(|cl, ch| {
                if let Some(cl) = cl {
                    prog_bar.set_length(cl);
                } else {
                    prog_bar.inc_length(ch.len() as u64);
                }
                prog_bar.inc(ch.len() as u64);
            })
            .instrument(
                info_span!("Downloading archive zip", id = archive.id, name = %archive.name),
            )
            .await?;

        let target_data_dir = self.data_dir_of_id(archive.id);
        std::fs::create_dir_all(&target_data_dir)?;

        msg_bar.set_prefix("Extracting");

        if let Err(e) = zip.extract(&target_data_dir) {
            tracing::error!(
                error = fuck_error(&e.into()),
                id = archive.id,
                name = %archive.name,
                "Failed to extract zip, trating this as a non-fatal error though"
            );
            return Ok(false);
        }

        msg_bar.set_prefix("Building symlinks");

        if let Err(e) = self.build_data_symlinks_for(archive) {
            tracing::error!(
                error = fuck_error(&e),
                id = archive.id,
                name = %archive.name,
                "Failed to create symlinks?, trating this as a non-fatal error though"
            );
        }

        msg_bar.set_prefix("Rendering");

        if let Err(e) = self.render_archive(archive) {
            tracing::error!(
                error = fuck_error(&e),
                id = archive.id,
                name = %archive.name,
                "Failed to render archive and generate symlinks?, trating this as a non-fatal error though"
            );
        }

        msg_bar.set_prefix("Indexing");

        self.sled_db
            .insert(archive.id.to_be_bytes(), serde_cbor::to_vec(&archive)?)?;
        self.searcher.add_archive(archive).await?;

        Ok(true)
    }

    pub fn render_archive(&self, archive: &Archive) -> Result<()> {
        let target_data_dir = self.data_dir_of_id(archive.id);
        let target_file = self.rendered_file_of_id(archive.id);

        if !target_file.exists() {
            std::fs::create_dir_all(target_file.parent().unwrap())?;

            self.generate_pdf_for(&archive.name, &target_data_dir, &target_file)?;
        }

        for tag in &archive.tags {
            let tag_file = self.rendered_file_for_archive_by_tag(&tag.name, &archive);
            std::fs::create_dir_all(tag_file.parent().unwrap())?;

            let src_file_v = target_file.clone().to_string_lossy().to_string();
            let dst_file_v = tag_file.to_string_lossy().to_string();

            symlink::symlink_file(&target_file, tag_file)
                .note("While symlinking the tag directory")
                .with_section(move || src_file_v.header("Source:"))
                .with_section(move || dst_file_v.header("Destination:"))?;
        }

        let artist_file = self.rendered_file_for_archive_by_artist(&archive);
        std::fs::create_dir_all(artist_file.parent().unwrap())?;
        symlink::symlink_file(&target_file, artist_file)?;

        Ok(())
    }

    pub fn generate_pdf_for(
        &self,
        name: &str,
        source_path: &Path,
        destination: &Path,
    ) -> Result<()> {
        let file_types = HashSet::<&'static OsStr>::from_iter([
            OsStr::new("png"),
            OsStr::new("jpg"),
            OsStr::new("jpeg"),
        ]);

        let images = walkdir::WalkDir::new(source_path)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .map_or(false, |ext| file_types.contains(ext))
            })
            .map(|entry| entry.path().to_owned())
            .collect::<Vec<_>>();

        let out_file = File::create(destination)?;

        let doc = PdfDocument::empty(name);

        for (i, image_path) in images.into_iter().enumerate() {
            let d_image = printpdf::image_crate::open(image_path)?;
            let image = printpdf::Image::from_dynamic_image(&d_image);
            let (page, layer) = doc.add_page(
                Px(d_image.width() as usize).into_pt(300.0).into(),
                Px(d_image.height() as usize).into_pt(300.0).into(),
                format!("Page {}", i + 1),
            );
            let layer_ref = doc.get_page(page).get_layer(layer);
            image.add_to_layer(layer_ref, printpdf::ImageTransform::default());
        }

        doc.save(&mut BufWriter::new(out_file))?;

        Ok(())
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
