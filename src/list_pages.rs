use crate::cli::{Cli, OutputFormat, Script};
use crate::common_tools::pages;
use clap::Parser;
use regex::{Regex, RegexBuilder};
use serde_json::{Map, Value};

#[derive(Parser)]
#[command(version = "0.1.0")]
pub struct ListPagesParameters {
    /// Defines the information requested from Crom, separated by spaces or commas.
    #[arg(long, short, default_value = "url wikidotInfo.title")]
    info: Vec<String>,
    /// Pages must include all following tags.
    #[arg(long, short = 'T', value_name = "TAGS...")]
    all_tags: Vec<String>,
    /// Pages must include one of the following tags.
    #[arg(long, short = 't', value_name = "TAGS...")]
    one_of_tags: Vec<String>,
    /// Searches within the pages attributed to the given author.
    #[arg(long, short)]
    author: Option<String>,
    /*#[arg(skip)]
    txt_output_format: String,*/
    /// Downloads the contents of each page from the HTML page.
    #[arg(long, default_value = "false")]
    content: bool,
    /// Downloads the sources of fragmented pages. Adds wikidotInfo.source to --info if not specified.
    #[arg(long, default_value = "false")]
    gather_fragments_sources: bool,
    /// Removes from the results all pages not containing all given regexes. Adds wikidotInfo.source to --info if not specified.
    #[arg(long, value_name = "REGEXES...")]
    source_contains: Vec<String>,
    /// Changes the behavior of --source-contains (removes pages not containing one of the given strings).
    #[arg(long, default_value = "false", requires = "source_contains")]
    source_contains_one: bool,
    /// Ignores case for --source-contains.
    #[arg(long, default_value = "false", requires = "source_contains")]
    source_contains_ignore_case: bool,
}

pub fn list_pages_subscript(global_data: &Cli, script_data: &ListPagesParameters, info: String) -> Vec<Value> {
    let source_contains: Vec<Regex> = script_data.source_contains.iter().map(|regex|
        RegexBuilder::new(regex.as_str()).case_insensitive(script_data.source_contains_ignore_case).build().unwrap_or_else(
            |e| panic!("Error: bad regex ({e})")
        )
    ).collect();

    let filter_and = script_data.all_tags.iter().fold("".to_string(), |acc, tag| {
        let tag_filter = format!("{{ tags: {{ eq: \"{tag}\" }} }}");
        if acc.is_empty() {
            tag_filter
        } else {
            format!("{{ _and: [{tag_filter}, {acc}] }}")
        }
    });

    let filter_or = script_data.one_of_tags.iter().fold("".to_string(), |acc, tag| {
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

    println!("Querying crom to list the pages…");
    pages(&global_data.verbose, global_data.site.as_ref().unwrap(), filter, script_data.author.as_ref(), info.to_string(), script_data.gather_fragments_sources, script_data.content).into_iter().filter(|page|
        page.get("wikidotInfo")
            .and_then(|wikidot_info| wikidot_info.get("source")
                .and_then(|source|
                    if source.is_null() {
                        eprintln!("Warning [Crom problem]: source is null. JSON: {page}");
                        Some(false)
                    } else {
                        source.as_str()
                            .and_then(|source| {
                                let source_contains_criteria = |criteria: &Regex| {
                                    criteria.is_match(source)
                                };
                                Some(if script_data.source_contains_one {
                                    source_contains.iter().any(source_contains_criteria)
                                } else {
                                    source_contains.iter().all(source_contains_criteria)
                                })
                            })
                    }
                )
            )
            .unwrap_or_else(|| {
                assert!(source_contains.is_empty(), "Error: source not found but --source-contains specified. JSON: {page}");
                true
            })
    ).collect()
}

#[derive(Debug)]
enum QueryTree {
    Node(String),
    MotherNode(String, Vec<QueryTree>),
    None
}

impl ToString for QueryTree {
    fn to_string(&self) -> String {
        match self {
            QueryTree::Node(node) => format!("{node},"),
            QueryTree::MotherNode(node, children) =>
                format!("{node} {{ {} }},",
                        children.iter().fold(String::new(), |acc, node|
                            acc + node.to_string().as_str()
                        )
                ),
            QueryTree::None => String::new()
        }
    }
}

fn _gciq_rec_fold(mut acc: Vec<QueryTree>, item: Vec<&str>) -> Vec<QueryTree> {
    match item.len() {
        0 => { acc.push(QueryTree::None); },
        1 => { acc.push(QueryTree::Node(item.first().unwrap().to_string())); },
        _ => {
            if acc.iter().any(|node| if let QueryTree::MotherNode(str, _) = node {str == item.first().unwrap()} else {false}) {
                acc = acc.into_iter().map(|node| if let QueryTree::MotherNode(str, vec) = node {
                   if str == item.first().unwrap().to_string() {
                       QueryTree::MotherNode(str, _gciq_rec_fold(vec, item[1..].to_vec()))
                   } else {
                       QueryTree::MotherNode(str, vec)
                   }
                } else {node}).collect();
            } else {
                acc.push(QueryTree::MotherNode(item.first().unwrap().to_string(), _gciq_rec_fold(Vec::new(), item[1..].to_vec())));
            }
        }
    }
    acc
}

fn _generate_crom_query_tree(info_list: Vec<&str>) -> Vec<QueryTree> {
    info_list.into_iter().map(|info| info.split(".").collect::<Vec<&str>>())
        .fold(Vec::new(), _gciq_rec_fold)
}

fn _generate_crom_information_query(info_list: Vec<&str>) -> String {
    _generate_crom_query_tree(info_list).into_iter().fold(String::new(), |str, node| str + node.to_string().as_str())
}

fn _filter_value(filters: &Vec<QueryTree>, value: Map<String, Value>) -> Map<String, Value> {
    value.into_iter()
    .filter_map(|(str, val) | {
        if filters.is_empty() {
            Some((str, val))
        } else {
            filters.iter().find(|filter|
                match filter {
                    QueryTree::MotherNode(filter_str, _) | QueryTree::Node(filter_str) => str == *filter_str,
                    _ => false
                }
            ).and_then(|corresponding_filter|
                if let Value::Object(obj) = val {
                    if let QueryTree::MotherNode(_, members) = corresponding_filter {
                        Some((str, Value::Object(_filter_value(members, obj))))
                    } else {
                        Some((str, Value::Object(obj)))
                    }
                } else {
                    Some((str, val))
                }
            )
        }
    }).collect()
}

pub fn list_pages(mut script_data: Cli) {

    {
        let params = match &mut script_data.script {
            Script::ListPages(p) => p
        };

        let source_str = "wikidotInfo.source".to_string();
        if (!params.source_contains.is_empty() || params.gather_fragments_sources) && !params.info.contains(&source_str) {
            params.info.push(source_str);
        }
    }

    let params = match &script_data.script {
        Script::ListPages(p) => p
    };

    let formatted_info = _generate_crom_information_query(params.info.iter().map(|s| s.as_str()).collect());

    let result: Vec<Value> = list_pages_subscript(&script_data, params, formatted_info);

    println!("{} result(s) found.", result.len());

    let path = script_data.output.path().clone();
    match script_data.output_format {
        OutputFormat::JSON => {serde_json::to_writer_pretty(script_data.output, &result)
            .unwrap_or_else(|e| panic!("Error writing into output file: {e}"));}
        OutputFormat::YAML => {serde_yaml::to_writer(script_data.output, &result)
            .unwrap_or_else(|e| panic!("Error writing into output file: {e}"));}
        OutputFormat::Text => {unimplemented!("Text output not yet implemented."); }
    }

    println!("Results written in file {}", path);



}