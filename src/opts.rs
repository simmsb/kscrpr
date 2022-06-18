use std::path::PathBuf;

use clap::{ArgEnum, Parser, Subcommand};
use once_cell::sync::OnceCell;
use url::Url;

/// Download stuff
#[derive(Parser)]
#[clap(about, version)]
pub struct Opts {
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
        #[clap(long, arg_enum, default_value_t = OutputAsType::Path, global = true)]
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
    Tag {
        #[clap(value_hint = clap::ValueHint::Other)]
        tag: String
    },
    /// Fetch an archive by id
    Id {
        #[clap(value_hint = clap::ValueHint::Other)]
        id: u32
    },
    // TODO: artist
}

#[derive(ArgEnum, Clone, Copy, PartialEq, Eq)]
#[clap(rename_all = "snake_case")]
pub enum OutputAsType {
    /// Show the path to the archive images by the id
    DataIdPath,
    /// Show the path to the archive images by artist/archive name
    DataPath,
    /// Show the path to the rendered archive by the id
    IdPath,
    /// Show the path to the rendered archive by artist/archive name
    Path,
    /// Show the id of each archive
    Id,
    /// Show the url of the archive
    Url,
    /// Show the name of the archive
    Name,
}

#[derive(Subcommand)]
pub enum GetCommand {
    /// List all archives with the given tags
    Tag {
        /// Display a ui for selecting from after filtering
        #[clap(long)]
        pick: bool,

        /// Open the rendered archive. Implies --pick
        #[clap(long)]
        open: bool,

        #[clap(min_values = 1, value_hint = clap::ValueHint::Other)]
        tags: Vec<String>,
    },
    /// Get an archive by id
    Id {
        #[clap(long)]
        open: bool,

        #[clap(value_hint = clap::ValueHint::Other)]
        id: u32,
    },
    /// Search for things
    Search {
        /// Default indexes to use for search terms that don't specify an index
        ///
        /// Specify an index with `index:term`, i.e. `tag:foo`
        #[clap(long, arg_enum, default_values = &["name", "artist", "parody", "tag"])]
        indexes: Vec<IndexType>,

        /// Maximum number of results to show
        #[clap(long)]
        max: Option<usize>,

        /// Display a ui for selecting from after filtering
        #[clap(long)]
        pick: bool,

        /// Open the rendered archive. Implies --pick
        #[clap(long)]
        open: bool,

        #[clap(value_hint = clap::ValueHint::Other)]
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
    /// Output the tag-organised rendered directory
    Tag,
    /// Output the artist organised rendered directory
    Artist,
    /// Output the data directory root
    Data,
    /// Output the metadata directory root
    Meta,
}

pub fn opts() -> &'static Opts {
    static INSTANCE: OnceCell<Opts> = OnceCell::new();
    INSTANCE.get_or_init(Opts::parse)
}
