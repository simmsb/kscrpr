use std::time::Duration;

use clap::IntoApp;
use color_eyre::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::archives::Archive;
use crate::config::{
    config, Command, Config, DirCommand, FetchCommand, GetCommand, IndexType, OutputAsType,
};
use crate::downloader::{by_id, fetch_tag_page};
use crate::filesystem::FileSystem;
use crate::utils::user_has_quit;

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
    let bar = MultiProgress::new();
    let msg_bar = bar.add(ProgressBar::new(1).with_style(
        ProgressStyle::with_template("{spinner:.green} {prefix:.cyan} {wide_msg}").unwrap(),
    ));
    msg_bar.enable_steady_tick(Duration::from_millis(200));
    let prog_bar = bar.add(
        ProgressBar::new(1).with_style(
            ProgressStyle::with_template("[{elapsed_precise}] {wide_bar:.cyan/blue} {pos:>}/{len}")
                .unwrap(),
        ),
    );
    prog_bar.enable_steady_tick(Duration::from_millis(200));
    bar.set_move_cursor(true);

    msg_bar.set_prefix("Clearing directories");
    msg_bar.tick();

    FileSystem::reset_tantivy_dir();
    FileSystem::reset_artists_dir();
    FileSystem::reset_rendered_dir();
    FileSystem::reset_tags_dir();
    let fs = FileSystem::open()?;

    prog_bar.set_length(fs.sled_db.len() as u64);
    prog_bar.set_position(0);

    for v in fs.sled_db.iter().values() {
        prog_bar.tick();
        let v = v?;
        let archive = serde_cbor::from_slice::<Archive>(&v)?;
        msg_bar.set_message(format!("Archive ({})[{}]", archive.id, archive.name));
        msg_bar.set_prefix("Indexing");
        fs.searcher.add_archive(&archive).await?;
        msg_bar.set_prefix("Building symlinks");
        fs.build_data_symlinks_for(&archive)?;
        msg_bar.set_prefix("Rendering");
        fs.render_archive(&archive)?;
        prog_bar.inc(1);

        if user_has_quit() {
            break;
        }
    }
    msg_bar.set_message("");
    msg_bar.set_prefix("Committing searcher");
    fs.searcher.commit().await?;

    msg_bar.finish_with_message("Done");
    prog_bar.finish();

    Ok(())
}

impl FetchCommand {
    pub async fn go(&self) -> Result<()> {
        let fs = FileSystem::open()?;

        match self {
            FetchCommand::Tag { tag } => {
                let mut new_archives = vec![];

                let bar = MultiProgress::new();
                let total_bar = bar.add(ProgressBar::new(0).with_style(
                    ProgressStyle::with_template("[{elapsed_precise:.yellow}] {wide_msg}").unwrap(),
                ));
                let msg_bar = bar.add(
                    ProgressBar::new(1).with_style(
                        ProgressStyle::with_template("{spinner:.green} {prefix:.cyan} {wide_msg}")
                            .unwrap(),
                    ),
                );
                total_bar.enable_steady_tick(Duration::from_millis(200));
                msg_bar.enable_steady_tick(Duration::from_millis(200));
                let prog_bar = bar.add(ProgressBar::new(1));
                prog_bar.enable_steady_tick(Duration::from_millis(200));
                bar.set_move_cursor(true);

                'outer: for page in 1.. {
                    total_bar.set_message(format!(
                        "[page {}] [newly downloaded {}]",
                        page,
                        new_archives.len()
                    ));
                    prog_bar.set_style(
                        ProgressStyle::with_template("{pos:>}/{len}")
                            .unwrap(),
                    );

                    if let Some(a) = fetch_tag_page(&fs, tag, page, &msg_bar, &prog_bar).await? {
                        prog_bar.set_style(
                            ProgressStyle::with_template(
                                "{bytes:>}/{total_bytes}",
                            )
                            .unwrap(),
                        );

                        for archive in a {
                            if fs.add_archive(&archive, false, &msg_bar, &prog_bar).await? {
                                new_archives.push(archive);
                            }

                            total_bar.set_message(format!(
                                "[page {}] [newly downloaded {}]",
                                page,
                                new_archives.len()
                            ));

                            if user_has_quit() {
                                fs.searcher.commit().await?;
                                break 'outer;
                            }
                        }

                        fs.searcher.commit().await?;
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

                let bar = MultiProgress::new();
                let msg_bar = bar.add(
                    ProgressBar::new(1).with_style(
                        ProgressStyle::with_template("{spinner:.green} {prefix:.cyan} {wide_msg}")
                            .unwrap(),
                    ),
                );
                msg_bar.enable_steady_tick(Duration::from_millis(200));
                let prog_bar = bar.add(
                    ProgressBar::new(1).with_style(
                        ProgressStyle::with_template(
                            "[{elapsed_precise}] {wide_bar:.cyan/blue} {bytes:>}/{total_bytes}",
                        )
                        .unwrap(),
                    ),
                );
                prog_bar.enable_steady_tick(Duration::from_millis(200));
                bar.set_move_cursor(true);

                if fs.add_archive(&archive, false, &msg_bar, &prog_bar).await? {
                    eprintln!("Archive was already downloaded");
                } else {
                    eprintln!("Added the following new archive:");
                    println!("{}", archive.name);
                }
                fs.searcher.commit().await?;
            }
        }

        Ok(())
    }
}

impl DirCommand {
    pub fn go(&self) -> Result<()> {
        let fs = FileSystem::open()?;

        let path = match self {
            DirCommand::Tag => fs.data_tag_dir(),
            DirCommand::Artist => fs.data_artist_dir(),
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
            OutputAsType::IdPath => println!("{}", fs.data_dir_of_id(doc.id).display()),
            OutputAsType::Path => println!(
                "{}",
                fs.data_dir_of_artist(&doc.artist).join(&doc.name).display()
            ),
            OutputAsType::Id => println!("{}", doc.id),
            OutputAsType::Url => println!("{}", doc.base_url),
            OutputAsType::Name => println!("{}", doc.name),
        }
    }
}
