use clap::IntoApp;
use color_eyre::Result;

use crate::archives::Archive;
use crate::config::{
    config, Command, Config, DirCommand, FetchCommand, GetCommand, IndexType, OutputAsType,
};
use crate::downloader::{by_id, fetch_tag_page};
use crate::filesystem::FileSystem;

pub async fn do_stuff() -> Result<()> {
    let config = config();

    config.command.go().await?;

    Ok(())
}

impl Command {
    pub async fn go(&self) -> Result<()> {
        match self {
            Command::Get { command, output_as } => command.go(*output_as).await,
            Command::Dir { command } => command.go(),
            Command::Fetch { command } => command.go().await,
            Command::Reindex => do_reindex().await,
            Command::Completion { shell } => {
                shell.generate(&mut Config::command(), &mut std::io::stdout());
                Ok(())
            }
        }
    }
}

async fn do_reindex() -> Result<()> {
    FileSystem::reset_tantivy_dir();
    FileSystem::reset_artists_dir();
    FileSystem::reset_tags_dir();
    let fs = FileSystem::open()?;

    for v in fs.sled_db.iter().values() {
        let v = v?;
        let archive = serde_cbor::from_slice::<Archive>(&v)?;
        fs.searcher.add_archive(&archive).await?;
        fs.build_symlinks_for(&archive)?;
    }

    Ok(())
}

impl FetchCommand {
    pub async fn go(&self) -> Result<()> {
        let fs = FileSystem::open()?;

        match self {
            FetchCommand::Tag { tag } => {
                let mut new_archives = vec![];
                for page in 1.. {
                    if let Some(a) = fetch_tag_page(&fs, tag, page).await? {
                        for archive in a {
                            if fs.add_archive(&archive, false).await? {
                                new_archives.push(archive);
                            }
                        }
                    } else {
                        break;
                    }
                }
                if new_archives.is_empty() {
                    eprintln!("Added no new archives");
                } else {
                    eprintln!("Added the following new archives:");
                    for archive in new_archives {
                        println!("{}", archive.name);
                    }
                }
            }
            FetchCommand::Id { id } => {
                let archive = by_id(*id).await?;

                if fs.add_archive(&archive, false).await? {
                    eprintln!("Archive was already downloaded");
                } else {
                    eprintln!("Added the following new archive:");
                    println!("{}", archive.name);
                }
            }
        }

        Ok(())
    }
}

impl DirCommand {
    pub fn go(&self) -> Result<()> {
        let fs = FileSystem::open()?;

        let path = match self {
            DirCommand::Tag => fs.tag_dir(),
            DirCommand::Artist => fs.artist_dir(),
            DirCommand::Data => fs.data_dir(),
            DirCommand::Meta => fs.meta_dir(),
        };

        println!("{}", path.display());

        Ok(())
    }
}

impl GetCommand {
    pub async fn go(&self, output_as: OutputAsType) -> Result<()> {
        let fs = FileSystem::open()?;

        match self {
            GetCommand::Tag { tags } => {
                let docs = fs.with_all_tags(tags).await?;

                if docs.is_empty() {
                    eprintln!("Nothing found :(");
                }

                for doc in docs {
                    output_as.print(&doc, &fs);
                }
            }
            GetCommand::Id { id } => {
                let doc = fs.fetch_doc(*id)?;

                output_as.print(&doc, &fs);
            }
            GetCommand::Search {
                query,
                indexes,
                max,
            } => {
                let indexes = indexes.iter().map(IndexType::str).collect::<Vec<_>>();
                let docs = fs.search(query, &indexes, *max).await?;

                if docs.is_empty() {
                    eprintln!("Nothing found :(");
                }

                for doc in docs {
                    output_as.print(&doc, &fs);
                }
            }
        }

        Ok(())
    }
}

impl OutputAsType {
    pub fn print(&self, doc: &Archive, fs: &FileSystem) {
        match self {
            OutputAsType::IdPath => println!("{}", fs.dir_of_id(doc.id).display()),
            OutputAsType::Path => println!(
                "{}",
                fs.dir_of_artist(&doc.artist).join(&doc.name).display()
            ),
            OutputAsType::Id => println!("{}", doc.id),
            OutputAsType::Url => println!("{}", doc.base_url),
            OutputAsType::Name => println!("{}", doc.name),
        }
    }
}
