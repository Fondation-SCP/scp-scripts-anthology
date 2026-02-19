extern crate core;

mod cli;
mod common_tools;
#[cfg(feature = "forum-dl")]
mod forum_dl;

#[cfg(feature = "list-pages")]
mod list_pages;

#[cfg(feature = "list-files")]
mod list_files;

use crate::forum_dl::forum_dl;
use clap::Parser;
use cli::Cli;
use cli::Script;
use crate::list_files::list_files;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let mut args = Cli::parse();

    if let Some(branch) = args.branch.as_ref() {
        args.site = Some(branch.get_url().to_string());
    }

    match args.script {
        #[cfg(feature = "list-pages")]
        Script::ListPages(_) => list_pages::run(args).await,
        #[cfg(feature = "forum-dl")]
        Script::ForumDl(_) => forum_dl(args).await,
        #[cfg(feature = "list-files")]
        Script::ListFiles(_) => list_files(args).await,
    }
}
