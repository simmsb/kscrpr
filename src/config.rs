use std::path::PathBuf;

use clap::{ArgEnum, Parser, Subcommand};
use once_cell::sync::OnceCell;
use url::Url;

/// Download stuff
#[derive(Parser)]
#[clap(about, version)]
pub struct Config {
    /// The base url to fetch from
    #[clap(env = "KSCRPR_BASE_URL", long)]
    pub base_url: Url,

    /// The directory to store data in
    #[clap(env = "KSCRPR_BASE_DIR", long, parse(from_os_str),
           default_value_os_t = dirs::document_dir().unwrap().join("kscrpr/"))]
    pub base_dir: PathBuf,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Get local archives matching criteria
    Get {
        #[clap(subcommand)]
        command: GetCommand,
        #[clap(arg_enum, default_value_t = OutputAsType::Path)]
        output_as: OutputAsType,
    },
    /// Fetch archives from the site
    Fetch {
        #[clap(subcommand)]
        command: FetchCommand,
    },
    /// Print a data dir
    Dir {
        #[clap(subcommand)]
        command: DirCommand,
    },
    /// Rebuild symlinks and the tantivy searcher
    Reindex,
    /// Generate shell completions
    Completion { shell: clap_complete_command::Shell },
}

#[derive(Subcommand)]
pub enum FetchCommand {
    /// Fetch all archives with the given tag
    Tag { tag: String },
    /// Fetch an archive by id
    Id { id: u32 },
    // TODO: artist
}

#[derive(ArgEnum, Clone, Copy, PartialEq, Eq)]
#[clap(rename_all = "snake_case")]
pub enum OutputAsType {
    IdPath,
    Path,
    Id,
    Url,
    Name,
}

#[derive(Subcommand)]
pub enum GetCommand {
    /// List all archives with the given tags
    Tag {
        #[clap(min_values = 1)]
        tags: Vec<String>,
    },
    /// Get an archive by id
    Id { id: u32 },
    /// Search for things
    Search {
        #[clap(long, short, arg_enum, default_values = &["name", "artist", "parody", "tag"])]
        indexes: Vec<IndexType>,
        #[clap(long, short)]
        max: Option<usize>,
        query: String,
    },
}

#[derive(ArgEnum, Clone, Copy, PartialEq, Eq)]
#[clap(rename_all = "snake_case")]
pub enum IndexType {
    Name,
    Artist,
    Parody,
    Tag,
}

impl IndexType {
    pub fn str(&self) -> &'static str {
        match self {
            IndexType::Name => "name",
            IndexType::Artist => "artist",
            IndexType::Parody => "parody",
            IndexType::Tag => "tag",
        }
    }
}

#[derive(Subcommand)]
pub enum DirCommand {
    /// Output the tag-organised directory root
    Tag,
    /// Output the artist organised directory root
    Artist,
    /// Output the data directory root
    Data,
    /// Output the metadata directory root
    Meta,
}

pub fn config() -> &'static Config {
    static INSTANCE: OnceCell<Config> = OnceCell::new();
    INSTANCE.get_or_init(Config::parse)
}
