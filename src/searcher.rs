use std::path::Path;

use color_eyre::Result;
use tantivy::collector::{DocSetCollector, TopDocs};
use tantivy::query::{BooleanQuery, Occur, Query, QueryParser, TermQuery};
use tantivy::schema::{Schema, INDEXED, STORED, TEXT};
use tantivy::{doc, Index, IndexWriter, Term};
use tokio::sync::Mutex;

use crate::archives::Archive;

pub struct Searcher {
    index: Index,
    writer: Mutex<IndexWriter>,
}

impl Searcher {
    pub fn new(base_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(base_dir)?;
        let mmap_directory = tantivy::directory::MmapDirectory::open(base_dir)?;
        let index = if Index::exists(&mmap_directory)? {
            Index::open(mmap_directory)?
        } else {
            let mut schema_builder = Schema::builder();
            schema_builder.add_u64_field("id", INDEXED | STORED);
            schema_builder.add_text_field("name", TEXT);
            schema_builder.add_text_field("artist", TEXT);
            schema_builder.add_text_field("parody", TEXT);
            schema_builder.add_text_field("tag", TEXT);

            let schema = schema_builder.build();

            Index::create_in_dir(base_dir, schema)?
        };

        let writer = Mutex::new(index.writer(3000000)?);

        Ok(Self { index, writer })
    }

    pub async fn add_archive(&self, archive: &Archive) -> Result<()> {
        let schema = self.index.schema();
        let id = schema.get_field("id").unwrap();
        let name = schema.get_field("name").unwrap();
        let artist = schema.get_field("artist").unwrap();
        let parody = schema.get_field("parody").unwrap();
        let tag = schema.get_field("tag").unwrap();

        let mut writer = self.writer.lock().await;

        let mut doc = doc!(
            id => archive.id as u64,
            name => archive.name.clone(),
            artist => archive.artist.clone(),
            parody => archive.parody.clone(),
        );

        for tag_v in &archive.tags {
            doc.add_text(tag, &tag_v.name);
        }

        writer.add_document(doc)?;

        writer.prepare_commit()?.commit_async().await?;

        Ok(())
    }

    pub async fn with_all_tags(&self, tags: &[String]) -> Result<Vec<u32>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();

        let schema = self.index.schema();
        let id_field = schema.get_field("id").unwrap();
        let tag_field = schema.get_field("tag").unwrap();

        let query_terms = tags
            .iter()
            .map(|tag| {
                (
                    Occur::Must,
                    Box::new(TermQuery::new(
                        Term::from_field_text(tag_field, tag),
                        tantivy::schema::IndexRecordOption::Basic,
                    )) as Box<dyn Query>,
                )
            })
            .collect();
        let query = BooleanQuery::new(query_terms);

        let all_docs = searcher.search(&query, &DocSetCollector)?;

        let mut matched_ids = Vec::with_capacity(all_docs.len());

        for doc_address in all_docs {
            let doc = searcher.doc_async(doc_address).await?;
            let doc_id = doc.get_first(id_field).unwrap().as_u64().unwrap();

            matched_ids.push(doc_id as u32);
        }

        Ok(matched_ids)
    }

    pub async fn search(
        &self,
        query: &str,
        default_indexes: &[&str],
        max: Option<usize>,
    ) -> Result<Vec<u32>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();

        let schema = self.index.schema();
        let default_indexes = default_indexes
            .iter()
            .map(|name| schema.get_field(name).unwrap())
            .collect::<Vec<_>>();

        let query_parser = QueryParser::for_index(&self.index, default_indexes);

        let query = query_parser.parse_query(query)?;

        if let Some(max) = max {
            let top_docs = searcher.search(&query, &TopDocs::with_limit(max))?;

            let id_field = schema.get_field("id").unwrap();

            let mut matched_ids = Vec::with_capacity(top_docs.len());

            for (_score, doc_address) in top_docs {
                let doc = searcher.doc_async(doc_address).await?;
                let doc_id = doc.get_first(id_field).unwrap().as_u64().unwrap();

                matched_ids.push(doc_id as u32);
            }

            Ok(matched_ids)
        } else {
            let all_docs = searcher.search(&query, &DocSetCollector)?;

            let id_field = schema.get_field("id").unwrap();

            let mut matched_ids = Vec::with_capacity(all_docs.len());

            for doc_address in all_docs {
                let doc = searcher.doc_async(doc_address).await?;
                let doc_id = doc.get_first(id_field).unwrap().as_u64().unwrap();

                matched_ids.push(doc_id as u32);
            }

            Ok(matched_ids)
        }
    }
}
