use std::fmt::Display;
use std::io;
use crate::cli::{Cli, Script};
use crate::common_tools::{pages, xml_escape};
use clap::Parser;
use regex::{Regex, RegexBuilder};
use serde_json::{Map, Value};
use chrono::DateTime;
use crate::common_tools;

#[derive(Parser)]
#[command(version = "0.2.0")]
pub struct ListPagesParameters {
    /// Defines the information requested from Crom, separated by spaces or commas.
    #[arg(long, short, default_value = "url wikidotInfo.title", num_args = 1..)]
    info: Vec<String>,
    /// Pages must include all following tags.
    #[arg(long, short = 'T', value_name = "TAG", num_args = 1..)]
    all_tags: Vec<String>,
    /// Pages must include one of the following tags.
    #[arg(long, short = 't', value_name = "TAG", num_args = 1..)]
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
    #[arg(long, value_name = "REGEX", num_args = 1..)]
    source_contains: Vec<String>,
    /// Changes the behavior of --source-contains (removes pages not containing one of the given strings).
    #[arg(long, default_value = "false", requires = "source_contains")]
    source_contains_one: bool,
    /// Ignores case for --source-contains.
    #[arg(long, default_value = "false", requires = "source_contains")]
    source_contains_ignore_case: bool,
    /// Sets default parameters to scrap the website for analysis with TXM. Overrides --content, --gather-fragment-sources, --format. Disables --source-contains.
    #[arg(long, default_value = "false")]
    txm: bool,
    /// Lists the files of listed pages
    #[arg(long, short, default_value = "false")]
    files: bool,
}

pub async fn list_pages_subscript(global_data: &Cli, script_data: &ListPagesParameters, info: String) -> Vec<Value> {
    let regexes_in_source: Vec<Regex> = script_data.source_contains.iter().map(|regex|
        RegexBuilder::new(regex.as_str()).case_insensitive(script_data.source_contains_ignore_case).build().unwrap_or_else(
            |e| panic!("Error: bad regex ({e})")
        )
    ).collect();

    let crom_recursive_query_builder = |operation| move |acc: String, tag| {
        let tag_filter = format!("{{ tags: {{ eq: \"{tag}\" }} }}");
        if acc.is_empty() {
            tag_filter
        } else {
            format!("{{ _{operation}: [{tag_filter}, {acc}] }}")
        }
    };

    let filter_and = script_data.all_tags.iter().fold("".to_string(), crom_recursive_query_builder("and"));

    let filter_or = script_data.one_of_tags.iter().fold("".to_string(), crom_recursive_query_builder("or"));

    let filter = match (filter_or.as_str(), filter_and.as_str()) {
        ("", "") => None,
        ("", yes) | (yes, "") => Some(yes.to_string()),
        (or, and) => Some(format!("{{ _and: [ {and}, {or} ] }}"))
    };



    println!("Querying crom to list the pages…");
    pages(global_data.verbose, global_data.site.as_ref().unwrap(), filter, script_data.author.as_ref(), info.to_string(), script_data.gather_fragments_sources, script_data.content).await.into_iter()
        .filter(|page|
            page.get("wikidotInfo")
                .and_then(|wikidot_info| wikidot_info.get("source"))
                .and_then(|source: &Value| {
                    /* Separated for clarity */
                    let matches_source_regexes = |source|{
                        let source_contains_criteria = |criteria: &Regex| criteria.is_match(source);
                        if script_data.source_contains_one {
                            regexes_in_source.iter().any(source_contains_criteria)
                        } else {
                            regexes_in_source.iter().all(source_contains_criteria)
                        }
                    };
                    if source.is_null() {
                        eprintln!("Warning [Crom problem]: source is null. JSON: {page}");
                        Some(false)
                    } else {
                        source.as_str().map(matches_source_regexes)
                    }
                }).unwrap_or_else(|| {
                    assert!(regexes_in_source.is_empty(), "Error: source not found but --source-contains specified. JSON: {page}");
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

impl Display for QueryTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            QueryTree::Node(node) => format!("{node},"),
            QueryTree::MotherNode(node, children) =>
                format!("{node} {{ {} }},",
                        children.iter().fold(String::new(), |acc, node|
                            acc + node.to_string().as_str()
                        )
                ),
            QueryTree::None => String::new()
        };
        write!(f, "{}", str)
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
        .filter_map(|(str, val)| {
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

fn _txm_output<W: io::Write>(mut output: W, data: &Vec<Value>) -> Result<(), io::Error> {
    let body = data.iter().map(|page| {
        let source = xml_escape(page.get("content").and_then(|content| content.as_str()).unwrap_or_else(|| panic!("Content absent but --txm used (internal error): {page}")));
        let wikidotinfo = page.get("wikidotInfo").unwrap_or_else(|| panic!("No wikidotInfo in data: {page}"));
        let title = xml_escape(wikidotinfo.get("title").and_then(|title| title.as_str()).unwrap_or_else(|| panic!("No title in data: {page}")));
        let rating = wikidotinfo.get("rating").and_then(|rating| rating.as_i64()).unwrap_or_else(|| panic!("No rating in data: {page}"));
        let tags = xml_escape(wikidotinfo.get("tags").unwrap_or_else(|| panic!("No tags in data but --txm used (internal error): {page}"))
            .as_array().unwrap_or_else(|| panic!("tags is no array: {page}"))
            .iter().map(|tag| tag.as_str().unwrap_or_default().to_string())
            .reduce(|acc, tag| acc + "," + tag.as_str()).unwrap_or_default().as_str());
        let date = wikidotinfo.get("createdAt")
            .and_then(|date| date.as_str())
            .and_then(|date_str| DateTime::parse_from_rfc3339(date_str).ok())
            .unwrap_or_else(|| panic!("date bad format: {page}"));
        let date_str = date.format("%Y-%m-%d").to_string();
        let time_str = date.format("%H:%M").to_string();
        let year_str = date.format("%Y").to_string();
        let month_str = date.format("%m").to_string();
        let weekday_str = date.format("%A").to_string();
        let hour_str = date.format("%H").to_string();
        let author = xml_escape(wikidotinfo.get("createdBy")
            .and_then(|cb| cb.get("name"))
            .and_then(|name| name.as_str())
            .unwrap_or_else(|| panic!("No author in data: {page}")));

        format!("<ecrit title=\"{title}\" rating=\"{rating}\" date=\"{date_str}\" time=\"{time_str}\" hour=\"{hour_str}\" year=\"{year_str}\" month=\"{month_str}\" weekday=\"{weekday_str}\" author=\"{author}\" tags=\"{tags}\">\n{source}\n</ecrit>\n",)
    }).reduce(|acc, item| acc + item.as_str()).unwrap_or_default();

    write!(output, "<?xml version=\"1.0\"?>\n<SCP>\n{body}\n</SCP>")
}

pub async fn list_pages(mut script_data: Cli) {

    {
        let params = match &mut script_data.script {
            Script::ListPages(p) => p,
            _ => panic!()
        };

        if params.txm {
            params.info = vec!["url",
                               "wikidotInfo.title",
                               "wikidotInfo.rating",
                               "wikidotInfo.tags",
                               "wikidotInfo.children.url",
                               "wikidotInfo.createdAt",
                               "wikidotInfo.createdBy.name"
            ].into_iter().map(|s| s.to_string()).collect();
            params.content = true;
        } else {
            let url_str = "url".to_string();
            if params.content && !params.info.contains(&url_str) {
                params.info.push(url_str);
            }

            let source_str = "wikidotInfo.source".to_string();
            if (!params.source_contains.is_empty() || params.gather_fragments_sources) && !params.info.contains(&source_str) {
                params.info.push(source_str);
            }

            let children_str = "wikidotInfo.children.url".to_string();
            if params.gather_fragments_sources && !params.info.contains(&children_str) {
                params.info.push(children_str);
            }
        }
    }

    let params = match &script_data.script {
        Script::ListPages(p) => p,
        _ => panic!()
    };

    let formatted_info = _generate_crom_information_query(params.info.iter().map(|s| s.as_str()).collect());

    let result: Vec<Value> = list_pages_subscript(&script_data, params, formatted_info).await;

    println!("{} result(s) found.", result.len());

    let path = script_data.output.path().clone();

    if !params.txm {
        common_tools::write_out(script_data, &result);
    } else {
        _txm_output(script_data.output, &result)
            .unwrap_or_else(|e| panic!("Error writing into output file: {e}"));
    }


    println!("Results written in file {}", path);

}

