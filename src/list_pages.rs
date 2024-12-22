use crate::common_tools::pages;
use crate::script_data::OutputFormat;
use crate::script_data::ScriptData;
use serde_json::{Map, Value};
use std::fs::File;
use regex::{Regex, RegexBuilder};

struct ListPagesParameters<'a> {
    source_contains: Vec<RegexBuilder>,
    source_contains_all: bool,
    source_contains_ignore_case: bool,
    all_tags: Vec<&'a str>,
    one_of_tags: Vec<&'a str>,
    author: Option<&'a str>,
    unread_args: Vec<(&'a str, &'a str)>,
    txt_output_format: &'a str,
    gather_fragments_sources: bool,
    download_content: bool,
}

impl<'a> ListPagesParameters<'a> {
    fn new() -> ListPagesParameters<'a> {
        ListPagesParameters {
            source_contains: Vec::new(),
            source_contains_all: true,
            source_contains_ignore_case: false,
            all_tags: Vec::new(),
            one_of_tags: Vec::new(),
            author: None,
            unread_args: Vec::new(),
            txt_output_format: "",
            gather_fragments_sources: false,
            download_content: false,
        }
    }

    pub fn source_contains(mut self, source_contains: RegexBuilder) -> Self {
        self.source_contains.push(source_contains);
        self
    }

    pub fn source_contains_all(mut self) -> Self {
        self.source_contains_all = true;
        self
    }

    pub fn source_contains_any(mut self) -> Self {
        self.source_contains_all = false;
        self
    }

    pub fn source_contains_ignore_case(mut self) -> Self {
        self.source_contains_ignore_case = true;
        self
    }

    pub fn all_tags(mut self, all_tags: Vec<&'a str>) -> Self {
        self.all_tags = all_tags;
        self
    }

    pub fn one_of_tags(mut self, one_of_tags: Vec<&'a str>) -> Self {
        self.one_of_tags = one_of_tags;
        self
    }

    pub fn author(mut self, author: Option<&'a str>) -> Self {
        self.author = author;
        self
    }

    pub fn unread_args(mut self, unread_args: (&'a str, &'a str)) -> Self {
        self.unread_args.push(unread_args);
        self
    }

    pub fn txt_output_format(mut self, txt_output_format: &'a str) -> Self {
        self.txt_output_format = txt_output_format;
        self
    }

    pub fn gather_fragments_sources(mut self) -> Self {
        self.gather_fragments_sources = true;
        self
    }

    pub fn download_content(mut self) -> Self {
        self.download_content = true;
        self
    }
}

pub fn list_pages_subscript(script_data: &mut ScriptData, info: String) -> Vec<Value> {
    let ListPagesParameters {
        source_contains,
        source_contains_all,
        source_contains_ignore_case,
        all_tags,
        one_of_tags,
        author,
        unread_args,
        txt_output_format: _, /* to implement later */
        gather_fragments_sources,
        download_content,
    } = script_data.other_args.iter()
        .fold(ListPagesParameters::new(), |lpp, (arg, value)| match *arg {
            "--all-tags" | "--all_tags" | "-T" => lpp.all_tags(value.split(" ").collect()),
            "--one-of-tags" | "--one_of_tags" | "-t" => lpp.one_of_tags(value.split(" ").collect()),
            "--author" | "-a" | "--user" | "-u" => lpp.author(Some(value)),
            "--source-contains" => lpp.source_contains(RegexBuilder::new(value)),
            "--source-contains-any" => lpp.source_contains_any(),
            "--source-contains-all" => lpp.source_contains_all(),
            "--source-contains-ignore-case" => lpp.source_contains_ignore_case(),
            "--text-output-format" => lpp.txt_output_format(value),
            "--gather-fragments-sources" => {
                assert!(info.contains("wikidotInfo.children.url"), "Error: --gather-fragments-sources must be used along with a --info requesting wikidotInfo.children.url");
                lpp.gather_fragments_sources()
            },
            "--content" => {
                assert!(info.contains("url"), "Error: --content needs --info requesting url.");
                lpp.download_content()
            },
            _ => lpp.unread_args((arg, value)),
        });

    let source_contains: Vec<Regex> = source_contains.into_iter().map(|mut regex_builder|
        regex_builder.case_insensitive(source_contains_ignore_case).build().unwrap_or_else(
            |e| panic!("Error: bad regex ({e})")
        )
    ).collect();

    assert!(source_contains.is_empty() || info.contains("source"), "Error: --source-contains must be used along with a --info requesting wikidotInfo.source");

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
    pages(&script_data.verbose, &script_data.site, filter, author, info.to_string(), gather_fragments_sources, download_content).into_iter().filter(|page|
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
                                Some(if source_contains_all {
                                    source_contains.iter().all(source_contains_criteria)
                                } else {
                                    source_contains.iter().any(source_contains_criteria)
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

pub fn list_pages(mut script_data: ScriptData) {
    let (filter, info, unread_args) = script_data.other_args.iter().fold((Vec::new(), "url wikidotInfo.title".to_string(), Vec::new()), |(text_format, info, unread_args), (arg, value)| match *arg {
        "--info" | "-i" => {
            assert!(script_data.output_path.is_some() || (value.contains("wikidotInfo.title") && value.contains("url")), "--output not defined (thus output is console out) but url or wikidotInfo.title are not requested by --info.");
            eprintln!("Warning: only the url and title will be shown in the terminal.");
            (text_format, _generate_crom_information_query(value.split(&[' ', ',']).collect()), unread_args)
        },
        "--output-filter" => if script_data.output_format != OutputFormat::Text {
            (_generate_crom_query_tree(value.split(&[' ', ',']).collect()), info, unread_args)
        } else {
            panic!("Error: --output-filter can't be used with --format txt");
        },
        _ => (text_format, info, unread_args.into_iter().chain(std::iter::once((*arg, *value))).collect())
    });

    script_data.other_args = unread_args;

    let result: Vec<Value> = list_pages_subscript(&mut script_data, info).into_iter().filter_map(|value|
        if let Value::Object(obj) = value {
            Some(Value::Object(_filter_value(&filter, obj)))
        } else {
            None
        }
    ).collect();

    script_data.other_args.iter().for_each(|(arg, _)| eprintln!("Warning: unknown parameter {arg}"));

    if let Some(path) = script_data.output_path {
        println!("{} result(s) found.", result.len());
        let file = File::create(&path).unwrap_or_else(|e| panic!("Error creating output file: {e}"));

        match script_data.output_format {
            OutputFormat::JSON => {serde_json::to_writer_pretty(file, &result)
                .unwrap_or_else(|e| panic!("Error writing into output file: {e}"));}
            OutputFormat::YAML => {serde_yaml::to_writer(file, &result)
                .unwrap_or_else(|e| panic!("Error writing into output file: {e}"));}
            OutputFormat::Text => {unimplemented!("Text output not yet implemented."); }
        }

        println!("Results written in file {path}");
    } else {
        let res_str = if result.is_empty() {
            "No results.".to_string()
        } else {
            result.iter().fold("".to_string(), |str, res| {
                let url = res.get("url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("[Unknown not found]");
                let title = res.get("wikidotInfo")
                    .and_then(|wikidot_info| wikidot_info.get("title")
                        .and_then(|title_info| title_info.as_str()))
                    .unwrap_or("[Unknown title]");
                format!("{str}\n{title} -- {url}")
            })
        };
        println!("Search results: {res_str}");
    }



}