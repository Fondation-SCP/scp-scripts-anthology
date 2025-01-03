use std::env;

mod script_data;
mod common_tools;
mod list_pages;
use script_data::ScriptData;
use script_data::OutputFormat;

fn main() {

    let args = env::args().collect::<Vec<String>>();

    let script = args.get(1)
        .unwrap_or_else(|| panic!("Error: expected script name. Use \"scp-script-anthology help\" to get more info.")).as_str();

    let (mut params, remains) = args[2..].into_iter().fold((Vec::new(), None), |(chain, str), arg| {
        match str {
            Some(param) => if arg.starts_with("-") {
                (chain.into_iter().chain(std::iter::once((param, ""))).collect(), Some(arg.as_str()))
            } else {
                (chain.into_iter().chain(std::iter::once((param, arg.as_str()))).collect(), None)
            }
            None => if arg.starts_with("-") {
                (chain, Some(arg.as_str()))
            } else {
                (chain, None)
            }
        }
    });

    if let Some(remains) = remains {
        params.push((remains, ""));
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
        match arg {
            "--branch" | "-b" => script_data.site = match value.to_lowercase().as_str() {
                "fr" | "french" => "http://fondationscp.wikidot.com/",
                "en" | "main" | "english" => "http://scp-wiki.wikidot.com/",
                "int" | "international" => "http://scp-int.wikidot.com/",
                br => panic!("Error: unknown branch {br}. Available branches: en, fr, int.")
            }.to_string(),
            "--site" | "-s" => script_data.site = value.to_string(),
            "--verbose" | "-v" => script_data.verbose = true,
            "--output" | "-o" => script_data.output_path = Some(value),
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

    match script {
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
        --info (-i): defines the information requested from crom. (TODO: list of available information) Must be used in combination with --output (can’t be directly printed in the console). Default: \"url  wikidotInfo.title\"\
        --content: downloads the content of each page directly from the HTML page (text content only).
        --gather-fragments-sources: if requesting for the page source, also gathers fragments' sources for fragmented pages (and/or content if --content is used). Must be used with a --info requesting wikidotInfo.children.url. Doesn't do anything without wikidotInfo.source or --content.
        --one-of-tags (-t): pages must include one of the following tags. Put them between quotation marks and separate each tag by a space.
        --output-filter: filter the information written in the output. Only the requested information will be written.
        --source-contains: keeps the pages that matches the given regex. Can be used multiple times; see --source-contains-any and --source-contains-all. Must be used with a --info requesting wikidotInfo.source.
        --source-contains-any: sets the source content filter to any. Pages whose source matches any of the --source-contains regexes will be kept.
        --source-contains-all: sets the source content filter to all. Pages whose source matches all of the --source-contains regexes will be kept.
        --source-contains-ignore-case: ignores the case for all --source-contains. Not activated by default."),
        script => panic!("Error: script {script} not found.")
    }
}
