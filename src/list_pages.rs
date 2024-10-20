use std::fs::File;
use serde_json::Value;
use crate::script_data::ScriptData;
use crate::common_tools::pages;
use crate::script_data::OutputFormat;

pub fn list_pages_subscript(script_data: &mut ScriptData, info: String) -> Vec<Value> {
    let (all_tags, one_of_tags, author, unread_args) = script_data.other_args.iter()
        .fold((Vec::new(), Vec::new(), None, Vec::new()), |(all_tags, one_of_tags, author, unread_args), (arg, value)| match arg.as_str() {
            "--all-tags" | "--all_tags" | "-T" => (value.split(" ").collect(), one_of_tags, author, unread_args),
            "--one-of-tags" | "--one_of_tags" | "-t" => (all_tags, value.split(" ").collect(), author, unread_args),
            "--author" | "-a" => (all_tags, one_of_tags, Some(value.clone()), unread_args),
            _ => (all_tags, one_of_tags, author, unread_args.into_iter().chain(std::iter::once((arg.clone(), value.clone()))).collect()),
        });

    let filter_and = all_tags.into_iter().fold("".to_string(), |acc, tag| {
        let tag_filter = format!("{{ tags: {{ eq: \"{tag}\" }} }}");
        if acc.is_empty() {
            tag_filter
        } else {
            format!("{{ _and: [{tag_filter}, {acc}] }}")
        }
    });

    let filter_or = one_of_tags.into_iter().fold("".to_string(), |acc, tag| {
        let tag_filter = format!("{{ tags: {{ eq: \"{tag}\" }} }}");
        if acc.is_empty() {
            tag_filter
        } else {
            format!("{{ _or: [{tag_filter}, {acc}] }}")
        }
    });

    let filter = match (filter_or.as_str(), filter_and.as_str()) {
        ("", "") => None,
        ("", yes) | (yes, "") => Some(yes.to_string()),
        (or, and) => Some(format!("{{ _and: [ {and}, {or} ] }}"))
    };

    script_data.other_args = unread_args;

    println!("Querying crom to list the pages…");
    pages(&script_data.verbose, &script_data.site, filter, author, info.to_string())
}

pub fn list_pages(mut script_data: ScriptData) {
    let (info, unread_args) = script_data.other_args.iter().fold(("url, wikidotInfo {title}".to_string(), Vec::new()), |(info, unread_args), (arg, value)| match arg.as_str() {
        "--info" | "-i" => if script_data.output_path.is_some() {
            (value.clone(), unread_args)
        } else {
            panic!("Error: --info must be used with --output; the format can’t be guessed and printed in the console.");
        },
        _ => (info, unread_args.into_iter().chain(std::iter::once((arg.clone(), value.clone()))).collect())
    });

    script_data.other_args = unread_args;

    let result = list_pages_subscript(&mut script_data, info);

    script_data.other_args.iter().for_each(|(arg, _)| eprintln!("Warning: unknown parameter {arg}"));

    if let Some(path) = script_data.output_path {
        println!("{} result(s) found.", result.len());
        let file = File::create(&path).unwrap_or_else(|e| panic!("Error creating output file: {e}"));

        match script_data.output_format {
            OutputFormat::JSON => {serde_json::to_writer_pretty(file, &result)
                .unwrap_or_else(|e| panic!("Error writing into output file: {e}"));}
            OutputFormat::YAML => {serde_yaml::to_writer(file, &result)
                .unwrap_or_else(|e| panic!("Error writing into output file: {e}"));}
        }

        println!("Results written in file {path}");
    } else {
        let res_str = if result.is_empty() {
            "No results.".to_string()
        } else {
            result.iter().fold("".to_string(), |str, res| {
                let url = res.get("url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("[URL not found]");
                let title = res.get("wikidotInfo")
                    .and_then(|wikidot_info| wikidot_info.get("title")
                        .and_then(|title_info| title_info.as_str()))
                    .unwrap_or("[No title]");
                format!("{str}\n{title} -- {url}")
            })
        };
        println!("Seach results: {res_str}");
    }



}