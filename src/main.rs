extern crate core;

mod cli;
mod common_tools;
mod forum_dl;
mod list_pages;
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
        Script::ListPages(_) => list_pages::run(args).await,
        Script::ForumDl(_) => forum_dl(args).await,
        Script::ListFiles(_) => list_files(args).await,
    }
}
