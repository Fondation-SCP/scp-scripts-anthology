mod cli;
mod common_tools;
mod forum_dl;
mod list_pages;

use crate::forum_dl::forum_dl;
use crate::list_pages::list_pages;
use clap::Parser;
use cli::Cli;
use cli::Script;
/*
Parse the parameters into a vector of parameters + arguments.
 */
fn _parse_parameters(args: &Vec<String>) -> Vec<(&str, &str)> {
    let (mut params, remains) =
        args[2..]
            .into_iter()
            .fold((Vec::new(), None), |(chain, str), arg| match str {
                Some(param) => {
                    if arg.starts_with("-") {
                        (
                            chain
                                .into_iter()
                                .chain(std::iter::once((param, "")))
                                .collect(),
                            Some(arg.as_str()),
                        )
                    } else {
                        (
                            chain
                                .into_iter()
                                .chain(std::iter::once((param, arg.as_str())))
                                .collect(),
                            None,
                        )
                    }
                }
                None => {
                    if arg.starts_with("-") {
                        (chain, Some(arg.as_str()))
                    } else {
                        (chain, None)
                    }
                }
            });

    if let Some(remains) = remains {
        params.push((remains, ""));
    }

    params
}

#[tokio::main]
async fn main() {
    let mut args = Cli::parse();

    if let Some(branch) = args.branch.as_ref() {
        args.site = Some(branch.get_url());
    }

    match args.script {
        Script::ListPages(_) => list_pages(args).await,
        Script::ForumDl(_) => forum_dl(args).await,
    }
}
