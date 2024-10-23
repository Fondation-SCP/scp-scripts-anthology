use std::env;

mod script_data;
mod common_tools;
mod list_pages;

use script_data::ScriptData;
use script_data::OutputFormat;

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
            other_args: Vec::new(),
            verbose: false,
            output_format: OutputFormat::YAML,
            output_path: None
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
            "--verbose" | "-v" => script_data.verbose = true,
            "--output" | "-o" => script_data.output_path = Some(value.to_string()),
            "--output-format" | "-f" => script_data.output_format = match value.to_lowercase().as_str() {
                "json" => OutputFormat::JSON,
                "yaml" | "yml" => OutputFormat::YAML,
                "text" | "txt" => /*OutputFormat::Text*/ unimplemented!("Text output format is not yet implemented."),
                format => panic!("Error: unknown format {format}. Accepted formats: yaml (default), json.")
            },
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
    --output (-o): file to save the output to.
    --output-format (-f): format of the output file. Available: yaml (default), json.

    List of available scripts:
    help: shows this text, and nothing else.
    list-pages: searches for pages in Crom’s database.
        list-pages parameters:
        --author (-a): search in the pages attributed to a specific author. If the username contains spaces, escape them or use quotation marks.
        --all-tags (-T): pages must include all following tags. Put them between quotation marks and separate each tag by a space.
        --one-of-tags (-t): pages must include one of the following tags. Put them between quotation marks and separate each tag by a space.
        --info (-i): defines the information requested from crom. (TODO: list of available information) Must be used in combination with --output (can’t be directly printed in the console). Default: \"url  wikidotInfo.title\"\
        --output-filter: filter the information written in the output. Only the requested information will be written.
        --source-contains: keeps the pages that contains the given string. Can be used multiple times; only the pages containing all strings will be kept. Must be used with a --info asking for at least the source."),
        script => panic!("Error: script {script} not found.")
    }
}
