use std::env;

mod sourced;
mod script_data;
mod common_tools;
mod list_pages;

use script_data::ScriptData;

fn main() {

    let script = env::args().collect::<Vec<String>>().get(1)
        .unwrap_or_else(|| panic!("Error: expected script name. Use \"scp-script-anthology help\" to get more info.")).clone();

    let (mut params, remains) = env::args().skip(2).fold((Vec::new(), None), |(chain, str), arg| {
        match str {
            Some(param) => if arg.starts_with("-") {
                (chain.into_iter().chain(std::iter::once((param, "".to_string()))).collect(), Some(arg))
            } else {
                (chain.into_iter().chain(std::iter::once((param, arg))).collect(), None)
            }
            None => if arg.starts_with("-") {
                (chain, Some(arg))
            } else {
                (chain, None)
            }
        }
    });

    if let Some(remains) = remains {
        params.push((remains, "".to_string()));
    }

    let script_data = params.into_iter().fold(
        ScriptData {
            site: "http://fondationscp.wikidot.com/".to_string(),
            list_all_pages: "system:list-all-pages".to_string(),
            other_args: Vec::new(),
            verbose: false
        },
        |mut script_data, (arg, value)| {
        match arg.as_str() {
            "--branch" | "-b" => script_data.site = match value.to_lowercase().as_str() {
                "fr" | "french" => "http://fondationscp.wikidot.com/",
                "en" | "main" | "english" => "http://scp-wiki.wikidot.com/",
                "int" | "international" => "http://scp-int.wikidot.com/",
                br => panic!("Error: unknown branch {br}. Available branches: en, fr, int.")
            }.to_string(),
            "--site" | "-s" => script_data.site = value.to_string(),
            "--page-list" => script_data.list_all_pages = value.to_string(),
            "--verbose" | "-v" => script_data.verbose = true,
            _ => script_data.other_args = script_data.other_args.into_iter().chain(std::iter::once((arg, value))).collect()
        }
            script_data
    });

    match script.as_str() {
        "list-pages" => list_pages::list_pages(script_data),
        "help" => println!("SCP Scripts Anthology, version 1.0
    Syntax: scp-scripts-anthology script_name parameters

    Global parameters:
    --branch (-b): sets the scp branch for the script. Branches available: en, fr, int.
    --site (-s): manually set the wikidot site url. Usefor for using scripts on sandboxes.
    --verbose (-v): prints Crom queries and their response.

    List of available scripts:
    help: shows this text, and nothing else.
    list-pages: searches for pages in Crom’s database.
        list-pages parameters:
        --author (-a): search in the pages attributed to a specific author. If the username contains spaces, escape them or use quotation marks.
        --all-tags (-T): pages must include all following tags. Put them between quotation marks and separate each tag by a space.
        --one-of-tags (-t): pages must include one of the following tags. Put them between quotation marks and separate each tag by a space."),
        script => panic!("Error: script {script} not found.")
    }
}
