use std::collections::HashMap;
use std::time::Duration;

use clap::IntoApp;
use color_eyre::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use itertools::Itertools;

use crate::archive::Archive;
use crate::filesystem::{self, FileSystem};
use crate::opts::{
    opts, Command, DirCommand, FetchCommand, GetCommand, IndexType, Opts, OutputAsType,
};
use crate::pick;
use crate::scrape::{by_id, fetch_tag_page};
use crate::utils::{self, user_has_quit};

pub async fn do_stuff() -> Result<()> {
    let opts = opts();

    opts.command.go().await?;

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
                shell.generate(&mut Opts::command(), &mut std::io::stdout());
                Ok(())
            }
            Command::Stats => do_stats().await,
        }
    }
}

async fn do_stats() -> Result<()> {
    let mut total = 0;
    let mut tag_counter = HashMap::new();

    let filesystem = filesystem::FileSystem::open()?;

    for archive in filesystem.fetch_all() {
        let archive = archive?;

        total += 1;
        for tag in &archive.tags {
            *tag_counter
                .raw_entry_mut()
                .from_key(&tag.name)
                .or_insert_with(|| (tag.name.clone(), 0))
                .1 += 1;
        }
    }

    println!("Total: {total}");

    let tags = tag_counter
        .into_iter()
        .sorted_by(|(k0, _), (k1, _)| std::cmp::Ord::cmp(k0, k1))
        .collect_vec();

    for (k, v) in tags {
        println!("{k}: {v}");
    }

    Ok(())
}

async fn do_reindex() -> Result<()> {
    ctrlc::set_handler(move || {
        utils::RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    })
    .unwrap();

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
        ctrlc::set_handler(move || {
            println!("C-c acknowleged, quitting soon!");
            utils::RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        })
        .unwrap();

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
                let prog_bar = bar.add(ProgressBar::new(1));
                total_bar.enable_steady_tick(Duration::from_millis(200));
                msg_bar.enable_steady_tick(Duration::from_millis(200));
                prog_bar.enable_steady_tick(Duration::from_millis(200));
                bar.set_move_cursor(true);

                'outer: for page in 1.. {
                    total_bar.set_message(format!(
                        "[page {}] [newly downloaded {}]",
                        page,
                        new_archives.len()
                    ));
                    prog_bar.set_style(ProgressStyle::with_template("{pos:>}/{len}").unwrap());

                    if let Some(a) = fetch_tag_page(&fs, tag, page, &msg_bar, &prog_bar).await? {
                        prog_bar.set_style(
                            ProgressStyle::with_template("{wide_bar} {bytes:>}/{total_bytes}")
                                .unwrap(),
                        );

                        for (archive, size) in a {
                            if fs
                                .add_archive(&archive, size, false, &msg_bar, &prog_bar)
                                .await?
                            {
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
                let (archive, size) = by_id(*id).await?;

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

                if fs
                    .add_archive(&archive, size, false, &msg_bar, &prog_bar)
                    .await?
                {
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
            DirCommand::Tag => fs.rendered_tag_dir(),
            DirCommand::Artist => fs.rendered_artist_dir(),
            DirCommand::Data => fs.data_dir(),
            DirCommand::Meta => fs.meta_dir(),
        };

        println!("{}", path.display());

        Ok(())
    }
}

fn do_pick(
    query: &str,
    docs: &[Archive],
    open: bool,
    output_as: OutputAsType,
    fs: &FileSystem,
) -> Result<()> {
    let selection = pick::do_pick(query, docs)?;

    let selected = match selection {
        Some(s) => s,
        None => return Ok(()),
    };

    if open {
        let path = fs.rendered_file_of_id(selected.id);
        opener::open(path)?;
    } else {
        output_as.print(selected, fs);
    }

    Ok(())
}

impl GetCommand {
    pub async fn go(&self, output_as: OutputAsType) -> Result<()> {
        let fs = FileSystem::open()?;

        match self {
            GetCommand::Tag { tags, pick, open } => {
                let docs = fs.with_all_tags(tags).await?;

                let pick = pick | open;

                if docs.is_empty() {
                    eprintln!("Nothing found :(");
                } else if pick {
                    let search = Itertools::intersperse(tags.iter().cloned(), ", ".to_owned())
                        .collect::<String>();
                    do_pick(&search, &docs, *open, output_as, &fs)?;
                } else {
                    for doc in docs {
                        output_as.print(&doc, &fs);
                    }
                }
            }
            GetCommand::Id { id, open } => {
                let doc = fs.fetch_doc(*id)?;

                if *open {
                    let path = fs.rendered_file_of_id(doc.id);
                    opener::open(path)?;
                } else {
                    output_as.print(&doc, &fs);
                }
            }
            GetCommand::Search {
                query,
                indexes,
                max,
                pick,
                open,
            } => {
                let indexes = indexes.iter().map(IndexType::str).collect::<Vec<_>>();
                let docs = fs.search(query, &indexes, *max).await?;

                let pick = pick | open;

                if docs.is_empty() {
                    eprintln!("Nothing found :(");
                } else if pick {
                    do_pick(query, &docs, *open, output_as, &fs)?;
                } else {
                    for doc in docs {
                        output_as.print(&doc, &fs);
                    }
                }
            }
        }

        Ok(())
    }
}

impl OutputAsType {
    pub fn print(&self, doc: &Archive, fs: &FileSystem) {
        match self {
            OutputAsType::DataIdPath => println!("{}", fs.data_dir_of_id(doc.id).display()),
            OutputAsType::DataPath => println!(
                "{}",
                fs.data_dir_of_artist(&doc.artist).join(&doc.name).display()
            ),
            OutputAsType::Id => println!("{}", doc.id),
            OutputAsType::Url => println!("{}", doc.base_url),
            OutputAsType::Name => println!("{}", doc.name),
            OutputAsType::IdPath => println!("{}", fs.rendered_file_of_id(doc.id).display()),
            OutputAsType::Path => {
                println!("{}", fs.rendered_file_for_archive_by_artist(doc).display())
            }
        }
    }
}
