mod cli;
mod crom;

use crate::cli::{Cli, Script};
use crate::common_tools;
use crate::common_tools::{close_browser, download_webpage_browser, file_list, open_browser, xml_escape, FutureIterator};
use crate::list_pages::crom::{Crom, QueryTree};
use chromiumoxide::Browser;
use chrono::DateTime;
pub(crate) use cli::ListPagesParameters;
use futures_util::{stream, StreamExt};
use regex::{Regex, RegexBuilder};
use scraper::Html;
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::{fs, io};
use itertools::Itertools;

pub async fn run(mut script_data: Cli) {
    let Script::ListPages(params) = &mut script_data.script else {
        panic!("Unreachable code")
    };
    params.apply_inferences();

    /* Immutable borrow replacing the mutable one */
    let Script::ListPages(params) = &script_data.script else {
        panic!("Unreachable code")
    };

    let html_folder = params.download_html.as_ref().map(Path::new);
    if html_folder.is_some_and(|folder| !folder.is_dir()) {
        panic!("--download-html: path given isn't a folder path or it doesn't exist.");
    }

    let formatted_info = QueryTree::from_vec(params.info.iter().map(|s| s.as_str()).collect())
        .into_iter().map(|qt| qt.to_string()).collect::<Box<[_]>>().concat();

    let result: Box<[Value]> = ListPages::new(&script_data, params, html_folder, formatted_info).execute().await;

    println!("{} result(s) found.", result.len());

    let path = script_data.output.path().clone();

    if !params.txm {
        common_tools::write_out(script_data, result.as_ref());
    } else {
        _txm_output(script_data.output, &result)
            .expect("Error writing into output file");
    }

    println!("Results written in file {}", path);
}

fn _txm_output(mut output: impl Write, data: &[Value]) -> Result<(), io::Error> {
    let body = data.iter().map(|page| {
        let source = xml_escape(page.get("content").and_then(Value::as_str).unwrap_or_else(|| panic!("Content absent but --txm used (internal error): {page}")));
        let wikidotinfo = page.get("wikidotInfo").unwrap_or_else(|| panic!("No wikidotInfo in data: {page}"));
        let title = xml_escape(wikidotinfo.get("title").and_then(Value::as_str).unwrap_or_else(|| panic!("No title in data: {page}")));
        let rating = wikidotinfo.get("rating").and_then(Value::as_i64).unwrap_or_else(|| panic!("No rating in data: {page}"));
        let tags = xml_escape(wikidotinfo.get("tags").unwrap_or_else(|| panic!("No tags in data but --txm used (internal error): {page}"))
            .as_array().unwrap_or_else(|| panic!("tags is no array: {page}"))
            .iter().map(|tag| tag.as_str().unwrap_or_default().to_string())
            .reduce(|acc, tag| acc + "," + tag.as_str()).unwrap_or_default().as_str());
        let date = wikidotinfo.get("createdAt")
            .and_then(Value::as_str)
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
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("No author in data: {page}")));

        format!("<ecrit title=\"{title}\" rating=\"{rating}\" date=\"{date_str}\" time=\"{time_str}\" hour=\"{hour_str}\" year=\"{year_str}\" month=\"{month_str}\" weekday=\"{weekday_str}\" author=\"{author}\" tags=\"{tags}\">\n{source}\n</ecrit>",)
    }).join("\n");

    write!(output, "<?xml version=\"1.0\"?>\n<SCP>\n{body}\n</SCP>")
}

/// Downloads all pages referenced by an entry (page + eventual children).
async fn _download_entry(page: &Value, children: Option<&[&Value]>, browser: Option<&Browser>) -> Box<[String]> {
    let title = page
        .get("wikidotInfo")
        .and_then(|wikidotinfo| wikidotinfo.get("title"));
    if let Some(title) = title {
        println!("Downloading webpage(s) of {title}");
    }

    // Merges the two ways of downloading in a single function to avoid duplicate code later.
    let download_webpage = async |url| {
        if let Some(browser) = browser {
            download_webpage_browser(url, browser).await
        } else {
            common_tools::download_webpage(url).await
        }
    };

    if let Some(children) = children {
        let newcontent = children.iter().map(async |fragment| {
            download_webpage(fragment.get("url").unwrap().as_str().unwrap()).await
        }).join_all().await.into_boxed_slice();

        if newcontent.iter().any(Option::is_none) {
            eprintln!("Warning: some fragments for page {} could not be downloaded or parsed.", page);
        }
        newcontent
    } else {
        Box::new([download_webpage(page.get("url").unwrap().as_str().unwrap()).await])
    }.into_iter().map(|x| x.unwrap_or(String::new())).collect() // Changes None Strings to empty Strings
}


#[derive(Debug)]
struct ListPages<'a> {
    verbose: bool,
    site: &'a str,
    filter: Option<String>,
    author: Option<&'a str>,
    requested_data: String,
    gather_fragments_sources: bool,
    download_content: bool,
    download_html: Option<&'a Path>,
    get_files: bool,
    source_contains_one: bool,
    threads: usize,
    regexes_in_source: Box<[Regex]>,
    crom: Crom
}

impl<'a> ListPages<'a> {
    fn new(
        global_data: &'a Cli,
        script_data: &'a ListPagesParameters,
        download_html: Option<&'a Path>,
        info: String,
    ) -> Self {
        let regexes_in_source: Box<[Regex]> = script_data
            .source_contains
            .iter()
            .map(|regex| {
                RegexBuilder::new(regex.as_str())
                    .case_insensitive(script_data.source_contains_ignore_case)
                    .build()
                    .expect("Bad regex")
            })
            .collect();

        let crom_recursive_query_builder = |operation|
            move |acc: String, tag| {
                let tag_filter = format!("{{ tags: {{ eq: \"{tag}\" }} }}");
                if acc.is_empty() {
                    tag_filter
                } else {
                    format!("{{ _{operation}: [{tag_filter}, {acc}] }}")
                }
            };

        let filter_and = script_data
            .all_tags
            .iter()
            .fold(String::new(), crom_recursive_query_builder("and"));

        let filter_or = script_data
            .one_of_tags
            .iter()
            .fold(String::new(), crom_recursive_query_builder("or"));

        let filter = match (filter_or.as_str(), filter_and.as_str()) {
            ("", "") => None,
            ("", yes) | (yes, "") => Some(yes.to_string()),
            (or, and) => Some(format!("{{ _and: [ {and}, {or} ] }}")),
        };

        Self {
            verbose: global_data.verbose,
            site: global_data.site.as_ref().unwrap(),
            filter,
            author: script_data.author.as_deref(),
            requested_data: info,
            gather_fragments_sources: script_data.gather_fragments_sources,
            download_content: script_data.content,
            download_html,
            threads: global_data.threads,
            get_files: script_data.files,
            source_contains_one: script_data.source_contains_one,
            regexes_in_source,
            crom: Crom::new(global_data.verbose)
        }
    }

    async fn execute(self) -> Box<[Value]> {
        if self.verbose {
            dbg!(&self);
        }

        let browser_handler = if self.get_files { Some(open_browser(false).await) } else { None };

        const _LOADING: fn(u64) -> String = |i| (0..i).map(move |n| if n+1 == i {"*"} else {"_"}).collect::<Box<[_]>>().concat();

        let _get_next_page = |next_page: Option<Option<String>>| {
            async {
                let next_page = next_page?;
                let resp = self._search_crom(&self.crom, next_page.as_deref()).await;
                let has_next_page = resp.get("pageInfo")
                    .and_then(|page_info| page_info.get("hasNextPage"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if has_next_page {
                    let next_page = resp.get("pageInfo")
                        .and_then(|page_info| page_info.get("endCursor"))
                        .and_then(|end_cursor| end_cursor.as_str())
                        .unwrap_or_else(|| panic!("No next page even though hasNextPage: {:?}", next_page))
                        .to_string();
                    print!("Fetching data from Crom… {:_<10}\r", _LOADING(next_page.as_bytes().iter().map(|b| *b as u64).sum::<u64>() % 10));
                    io::stdout().flush().unwrap();
                    Some((resp, Some(Some(next_page))))
                } else {
                    println!("Download finished.");
                    Some((resp, None))
                }
            }
        };

        println!("Querying crom to list the pages…");
        let crom_responses = stream::unfold(Some(None), _get_next_page).collect::<Vec<_>>().await;

        let mut pages = crom_responses.into_iter().flat_map(self._get_pages()).collect::<Box<[_]>>();

        println!("{} pages found.", pages.len());

        const _HAS_SOURCE: fn(&Value) -> bool = |page: &Value| page
            .get("wikidotInfo")
            .and_then(|wikidot_info| wikidot_info.get("source"))
            .is_some();

        if self.gather_fragments_sources && pages.iter().any(_HAS_SOURCE) {
            self._gather_fragments_sources(pages.as_mut()).await;
        }

        if self.download_content || self.get_files || self.download_html.is_some() {
            let htmls = self._download_html(browser_handler.as_ref().map(|(a, _)| a), pages.as_mut()).await;

            if let Some(folder) = self.download_html.and_then(|h| h.to_str()) {
                self._write_htmls(folder, pages.as_ref(), htmls.as_ref()).await;
            }

            if self.download_content || self.get_files {
                let parsed_htmls = htmls.iter().map(String::as_str)
                    .map(|s| async {Html::parse_document(s)})
                    .into_future_iter()
                    .buffered(self.threads)
                    .collect::<Vec<_>>()
                    .await;

                if self.download_content {
                    parsed_htmls.iter()
                        .map(|html| async { common_tools::parse_content(html) })
                        .into_future_iter().buffered(self.threads).collect::<Vec<_>>().await.into_iter()
                        .map(Option::unwrap_or_default)
                        .zip(pages.iter_mut())
                        .for_each(|(html, page)| {
                            if let Some(page) = page.as_object_mut() {
                                page.insert("content".to_string(), Value::String(html));
                            }
                        });
                }

                if self.get_files {
                    parsed_htmls.iter().map(|html| async {file_list(html)})
                        .into_future_iter().buffered(self.threads).collect::<Vec<_>>().await
                        .into_iter()
                        .zip(pages.iter_mut())
                        .for_each(|(file_list, page)| {
                            if let Some(page) = page.as_object_mut() {
                                page.insert("files".to_string(), serde_json::to_value(file_list).unwrap());
                            }
                        });
                }
            }
        }

        if let Some(browser_handler) = browser_handler {
            close_browser(browser_handler).await;
        }

        let _source_contains = |page: &Value| {
            page.get("wikidotInfo")
                .and_then(|wikidot_info| wikidot_info.get("source"))
                .and_then(|source: &Value| {
                    if source.is_null() {
                        eprintln!("Warning [Crom problem]: source is null. JSON: {page}");
                        return Some(false);
                    }

                    /* Separated for clarity */
                    let matches_source_regexes = |source| {
                        let source_contains_criteria = |criteria: &Regex| criteria.is_match(source);
                        if self.source_contains_one {
                            self.regexes_in_source.iter().any(source_contains_criteria)
                        } else {
                            self.regexes_in_source.iter().all(source_contains_criteria)
                        }
                    };
                    source.as_str().map(matches_source_regexes)
                })
                .unwrap_or_else(|| {
                    assert!(
                        self.regexes_in_source.is_empty(),
                        "Error: source not found but --source-contains specified. JSON: {page}"
                    );
                    true
                })
        };

        pages.into_iter().filter(_source_contains).collect()
    }

    async fn _search_crom(&self, crom: &Crom, after: Option<&str>) -> Value {
        let query = Crom::build_crom_query(self.site, self.filter.as_deref(), self.author, &self.requested_data, after);
        let mut response = crom.query(query.as_str()).await;
        if self.verbose {
            println!("Query: {query}");
            println!("Response: {response}");
        }
        response.get_mut("data")
            .and_then(|data|
                /* Response structure is different if querying for a specific user or generally */
                if self.author.is_some() {
                    data.get_mut("user")
                        .and_then(|user| user.get_mut("attributedPages"))
                } else {
                    data.get_mut("pages")
                }.map(Value::take)
            ).unwrap_or_else(|| panic!("Error in JSON response from CROM: {}\nQuery: {query}", response))
    }

    fn _get_pages(&self) -> impl Fn(Value) -> Box<[Value]> {
        |mut crom_response: Value| {
            let err_message = format!("Error in JSON response from CROM: {}", crom_response);

            crom_response.get_mut("edges")
                .and_then(|edges| edges.as_array_mut())
                .unwrap_or_else(|| panic!("{err_message}"))
                .iter_mut()
                .map(|edge| {
                    edge.get_mut("node")
                        .unwrap_or_else(|| panic!("{err_message}"))
                })
                .inspect(|page| {
                    println!(
                        "{}",
                        page.get("url")
                            .and_then(|url| url.as_str())
                            .unwrap_or("Invalid URL")
                    )
                })
                .map(Value::take)
                .collect::<Box<[_]>>()
        }
    }

    async fn _gather_fragments_sources(&self, pages: &mut [Value]) {
        let _gather_fragments_sources = async |page: &Value| {
            let children = Self::_list_children(page);
            children
                .iter()
                .map(|fragment| self.crom._get_fragment_source(fragment))
                .join_all().await
                .join("\n")
        };

        pages.iter_mut().map(|page| async {
            let new_source = _gather_fragments_sources(page).await;
            let Some(old_source) = page.get_mut("wikidotInfo")
                .and_then(|wi| wi.get_mut("source")) else {
                return;
            };
            *old_source = Value::String(new_source);
        }).join_all().await;
    }

    async fn _download_html(&self, browser: Option<&Browser>, pages: &[Value]) -> Box<[String]> {
        pages.iter()
            .map(|page| (page, if self.gather_fragments_sources { Some(Self::_list_children(page)) } else { None }))
            .map(async |(page, children)| _download_entry(
                page,
                children.as_ref().map(Box::as_ref),
                browser
            ).await.join("\n"))
            .into_future_iter()
            .buffered(self.threads)
            .collect::<Vec<_>>().await.into_boxed_slice()
    }

    async fn _write_htmls(&self, folder: &str, pages: &[Value], htmls: &[String]) {
        let pages_names = pages.iter()
            .map(|page|
                page.get("url")
                    .and_then(|url| url.as_str())
                    .and_then(|url| url.split("/").last())
                    .expect("Malformed url?")
            ).collect::<Box<[_]>>();
        htmls.iter()
            .zip(pages_names.into_iter())
            .map(async |(html, page_name)|
                fs::File::open(format!("{folder}/{page_name}.html"))?
                    .write_all(html.as_bytes())
            )
            .into_future_iter()
            .buffer_unordered(self.threads)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .filter_map(|r: Result<_, _>| r.err())
            .for_each(|e| eprintln!("Could not write file: {:#?}", e));
    }

    fn _list_children(page: &Value) -> Box<[&Value]> {
        page.get("wikidotInfo")
            .and_then(|wikidotinfo| wikidotinfo.get("children"))
            .and_then(|children| children.as_array())
            .map(|children| {
                children
                    .iter()
                    .filter(|child| {
                        child
                            .get("url")
                            .and_then(|url| url.as_str())
                            .is_some_and(|url| url.contains("fragment:"))
                    })
                    .rev()
                    .collect()
            })
            .unwrap_or_default()
    }
}
