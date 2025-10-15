use std::io;
use std::io::Write;
use chromiumoxide::{Browser, BrowserConfig};
use crate::cli::{Cli, OutputFormat};
use futures_util::future::join_all;
use futures_util::{FutureExt, StreamExt};
use reqwest::header::USER_AGENT;
use rpassword::read_password;
use scraper::{ElementRef, Html, Selector};
use serde::{Serialize};
use serde_json::Value;
use tokio::task::JoinHandle;

async fn _wait_for_ratelimit(client: &reqwest::Client, crom_url: &str) {
    let mut retries = 0;
    let rate_limit_request = "query {rateLimit{remaining, resetAt}}";

    loop {
        let response = loop {
            assert!(retries < 5, "Too many failed attemps: giving up.");
            let response = client
                .post(crom_url)
                .header(USER_AGENT, "ScpScriptAnthology/1.0")
                .json(&serde_json::json!({"query": rate_limit_request}))
                .send()
                .await;

            match response {
                Ok(r) => break r,
                Err(e) => {
                    eprintln!("Request error: {e}.");
                    retries += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                }
            }
        };

        retries = 0;

        let json_res: Value = response
            .json()
            .await
            .expect("Recieved data is not JSON? Error");

        let remaining = json_res.get("data").and_then(|data| {
            data.get("rateLimit").and_then(|ratelimit| {
                ratelimit
                    .get("remaining")
                    .and_then(|remaining| remaining.as_u64())
            })
        });

        match (remaining, json_res.get("errors")) {
            (Some(0), _) => {
                println!("Rate limited by Crom. Waiting 5 minutes.");
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
            }
            (None, Some(errors)) => {
                eprintln!("Warning: Crom might be flooded! Waiting 30 seconds.\n{errors}");
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                eprintln!("Retrying.");
            }
            (None, None) => panic!(
                "No ratelimit nor errors founds in CROM response: {}",
                json_res.to_string()
            ),
            _ => break, // Not rate limited
        }
    }
}

pub async fn query_crom(request: &String) -> Value {
    let crom_url = "https://api.crom.avn.sh/graphql";
    let mut retries = 0;
    let client = reqwest::Client::new();
    loop {
        _wait_for_ratelimit(&client, &crom_url).await;
        let res: Value = loop {
            assert!(retries < 5, "Too many failed attemps: giving up.");
            let response = client
                .post(crom_url)
                .header(USER_AGENT, "ScpScriptAnthology/1.0")
                .json(&serde_json::json!({"query": request}))
                .send()
                .await;
            match response {
                Err(e) => {
                    eprintln!("Request error: {e}. Retrying in 10 seconds.");
                    retries += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                }
                Ok(response) => {
                    break response.json().await.expect("Recieved data is not JSON?");
                }
            }
        };

        if let Some(errors) = res.get("errors") {
            eprintln!("Crom returned error(s): {errors}. Retrying.");
        } else {
            break res;
        }
    }
}

/// Exctracts the main content of a Wikidot webpage
fn _parse_content(doc: &Html) -> Option<String> {
    let page_content_sel = Selector::parse("#page-content").unwrap();
    let Some(doc) = doc.select(&page_content_sel).next() else {
        eprintln!("#page-content not found.");
        return None;
    };

    let deletion_selectors = vec![
        Selector::parse(".creditRate"),
        Selector::parse(".code"),
        Selector::parse(".footer-wikiwalk-nav"),
    ]
        .into_iter()
        .map(|selector| selector.unwrap());

    let delete_element =
        |collector: String, element: ElementRef| collector.replace(&element.html(), "");

    Some(
        Html::parse_fragment(
            deletion_selectors
                .fold(doc.html(), |collector, selector| {
                    doc.select(&selector).fold(collector, delete_element)
                })
                .as_str(),
        )
            .root_element()
            .text()
            .collect(),
    )
}

#[derive(Debug)]
struct File {
    name: String,
    file_type: String,
    size: i32
}

impl Into<Value> for File {
    fn into(self) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("name".to_string(), Value::String(self.name));
        obj.insert("file_type".to_string(), Value::String(self.file_type));
        obj.insert("size".to_string(), Value::Number(self.size.into()));
        Value::Object(obj)
    }
}

fn _parse_file_size(str: &str) -> f32 {
    let mut parts = str.split(" ");
    parts.next().unwrap_or_else(|| {eprintln!("Can't split file size."); ""}).parse::<f32>().map(|size|
        size * match parts.next() {
            Some("kB") => 1000.,
            Some("MB") => 10000000.,
            Some("Bytes") => 1.,
            Some(u) => {
                eprintln!("Unknown unit {u}.");
                1.
            },
            None => {
                eprintln!("No unit found: {str}");
                1.
            }
    }).unwrap_or_else(|err| {eprintln!("Can't parse size: {str}: {err}."); 0.})
}

fn _file_list(doc: &Html) -> Vec<File> {
    let file_list_selector = Selector::parse("table.page-files tbody").unwrap();
    let Some(file_list) = doc.select(&file_list_selector).next() else {
        return vec![]; // No files
    };

    file_list.children().filter_map(ElementRef::wrap).map(|line| {
        let cells = line.children().filter_map(ElementRef::wrap).collect::<Vec<_>>();
        File {
            name : cells.get(0)
                .and_then(|cell| cell.children().filter_map(ElementRef::wrap).next())
                .map(|cell| cell.inner_html())
                .unwrap_or(String::new()),
            file_type : cells.get(1)
                .and_then(|cell| cell.children().filter_map(ElementRef::wrap).next())
                .map(|cell| cell.inner_html())
                .unwrap_or(String::new()),
            size: cells.get(2)
                .map(|cell| _parse_file_size(cell.inner_html().trim()))
                .unwrap_or(0.).round() as i32,
        }
    }).collect()

}

/// Downloads a singular webpage.
async fn _download_webpage(url: &str) -> Option<String> {
    /* Downloading html */
    let mut retries = -1;
    let client = reqwest::Client::new();
    loop {
        match retries {
            -1 => {
                retries = 0;
            }
            i if i <= 5 => {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                retries += 1;
            }
            _ => {
                eprintln!("Error while downloading {url}: 5 failed attempts. Giving up.");
                break None;
            }
        }

        let response = match client
            .get(url)
            .header(USER_AGENT, "ScpScriptAnthology/1.0")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Download error: {e}.");
                continue;
            }
        };

        match response.text().await {
            Ok(h) => break Some(h),
            Err(e) => {
                eprintln!("Format error: {e}.");
                continue;
            }
        }
    }
}

async fn _download_webpage_browser(url: &str, browser: &Browser) -> Option<String> {
    // Put it in a closure so I can use the ? macro for readability.
    let f = async || {
        let page = browser.new_page(url).await?;
        page.evaluate("WIKIDOT.page.listeners.filesClick();").await?;
        let mut youre_taking_too_long = 30;
        loop {
            youre_taking_too_long -= 1;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            // If the action area has content
            if !page.find_element("#action-area").await?.find_elements("a").await?.is_empty() || youre_taking_too_long == 0 {
                break;
            }
        }
        if youre_taking_too_long == 0 {
            eprintln!("[WARNING] No action area found for {url}. Your login attempt may might have been unsuccessful.");
        }
        let html = page.wait_for_navigation().await?.content().await?;
        page.close().await?;

        Ok::<_, Box<dyn std::error::Error>>(html)
    };

    match f().await {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!("Warning: couldn't download with browser page. Cause: {e}.");
            None
        }
    }
}

fn _list_children(page: &Value) -> Vec<&Value> {
    page.get("wikidotInfo")
        .and_then(|wikidotinfo| wikidotinfo.get("children"))
        .and_then(|children| children.as_array())
        .map(|children| {
            children
                .into_iter()
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

async fn _crom_pages(
    verbose: bool,
    site: &String,
    filter: Option<String>,
    author: Option<&String>,
    requested_data: String,
    gather_fragments_sources: bool,
    download_content: bool,
    get_files: bool,
    browser: Option<&Browser>,
    after: Option<&str>,
) -> Vec<Value> {
    let query = _build_crom_query(&site, &filter, &author, &requested_data, &after);
    let response = query_crom(&query).await;
    if verbose {
        println!("Query: {query}");
        println!("Response: {response}");
    }
    let response_parsed = response.get("data")
        .and_then(|data|
            /* Response structure is different if querying for a specific user or generally */
            if author.is_some() {
                data.get("user")
                    .and_then(|user| user.get("attributedPages"))
            } else {
                data.get("pages")
            }
        ).expect(format!("Error in JSON response from CROM: {}\nQuery: {query}", response).as_str());

    let mut pages: Vec<Value> = response_parsed
        .get("edges")
        .and_then(|edges| edges.as_array())
        .expect(format!("Error in JSON response from CROM: {}", response_parsed).as_str())
        .iter()
        .map(|edge| {
            edge.get("node")
                .expect(format!("Error in JSON response from CROM: {}", edge).as_str())
                .clone()
        })
        .inspect(|page| {
            println!(
                "{}",
                page.get("url")
                    .and_then(|url| url.as_str())
                    .unwrap_or("Invalid URL")
            )
        })
        .collect();

    if gather_fragments_sources || download_content || get_files {
        join_all(
            pages.iter_mut().map(|page| {
                _add_new_data(verbose, gather_fragments_sources, download_content, get_files, browser, page)
            }),
        )
        .await;
    }

    let page_info = response_parsed
        .get("pageInfo")
        .expect(format!("Error in JSON response from CROM: {}", response_parsed).as_str());

    let has_next_page = page_info
        .get("hasNextPage")
        .and_then(|has_next_page| has_next_page.as_bool())
        .expect(format!("Error in JSON response from CROM: {}", page_info).as_str());

    if has_next_page {
        let next_page = page_info
            .get("endCursor")
            .and_then(|end_cursor| end_cursor.as_str())
            .expect(format!("Error in JSON response from CROM: {}", page_info).as_str());
        pages
            .into_iter()
            .chain(
                Box::pin(_crom_pages(
                    verbose,
                    site,
                    filter,
                    author,
                    requested_data,
                    gather_fragments_sources,
                    download_content,
                    get_files,
                    browser,
                    Some(next_page),
                ))
                .await,
            )
            .collect()
    } else {
        pages
    }
}

async fn _add_new_data(
    verbose: bool,
    gather_fragments_sources: bool,
    download_content: bool,
    get_files: bool,
    browser: Option<&Browser>,
    page: &mut Value,
) {
    assert!(
        page.is_object(),
        "Error in JSON response from CROM (not an object): {}",
        page
    );
    let children: Vec<_> = if gather_fragments_sources {
        _list_children(page)
    } else {
        Vec::new()
    };
    let mut newsource = None;
    if gather_fragments_sources
        && page
        .get("wikidotInfo")
        .and_then(|wikidot_info| wikidot_info.get("source"))
        .is_some()
    {
        newsource = join_all(
            children
                .iter()
                .map(|fragment| _gather_fragment_source(verbose, fragment)),
        )
            .await
            .into_iter()
            .reduce(|collector, part| collector + "\n" + part.as_str());
    }

    if download_content || get_files {
        let pages = _download_entry(page, children, browser).await;
        if let Some(html) = pages.get(0).map(|s| Html::parse_document(s.as_str())) {
            if download_content {
                let newcontent = _parse_content(&html).unwrap_or(String::new());
                page.as_object_mut().unwrap().insert("content".to_string(), Value::String(newcontent));
            }

            if get_files {
                let file_list: Vec<Value> = _file_list(&html).into_iter().map(|x| x.into()).collect();
                page.as_object_mut().unwrap().insert("files".to_string(), Value::Array(file_list));
            }

        } else {
            eprintln!("Warning: could not download or parse content for page {}.", page);
        }
    }

    /* Done last to avoid creating a mutable reference to the page before
    now possible because the "children" reference won't be used anymore */
    if let Some(newsource) = newsource {
        if let Some(oldsource) = page
            .get_mut("wikidotInfo")
            .and_then(|wikidot_info| wikidot_info.as_object_mut())
            .and_then(|wikidot_info| wikidot_info.get_mut("source"))
        {
            *oldsource = Value::String(newsource.to_string());
        }
    }
}

/// Downloads all pages referenced by an entry (page + eventual children).
async fn _download_entry(page: &Value, children: Vec<&Value>, browser: Option<&Browser>) -> Vec<String> {
    let title = page
        .get("wikidotInfo")
        .and_then(|wikidotinfo| wikidotinfo.get("title"));
    if let Some(title) = title {
        println!("Downloading webpage(s) of {title}");
    }

    // Merges the two ways of downloading in a single function to avoid duplicate code later.
    let download_webpage = async |url| {
        if let Some(browser) = browser {
            _download_webpage_browser(url, browser).await
        } else {
            _download_webpage(url).await
        }
    };

    if !children.is_empty() {
        let newcontent = join_all(children.iter().map(async |fragment| {
            download_webpage(fragment.get("url").unwrap().as_str().unwrap()).await
        }))
        .await;
        if newcontent.iter().any(|frag| frag.is_none()) {
            eprintln!("Warning: some fragments for page {} could not be downloaded or parsed.", page);
        }
        newcontent
    } else {
        vec![download_webpage(page.get("url").unwrap().as_str().unwrap()).await]
    }.into_iter().map(|x| x.unwrap_or(String::new())).collect() // Changes None Strings to empty Strings
}

async fn _gather_fragment_source(verbose: bool, fragment: &&Value) -> String {
    let query = &format!(
        "
                        query {{
                            page(url:{}){{
                                wikidotInfo {{ source }}
                            }}
                        }}
                   ",
        fragment.get("url").unwrap()
    );
    if verbose {
        println!("Query: {query}");
    }
    let response = query_crom(query).await;
    if verbose {
        println!("Response: {response}");
    }
    response
        .get("data")
        .and_then(|d| d.get("page"))
        .and_then(|p| p.get("wikidotInfo"))
        .and_then(|wi| wi.get("source"))
        .and_then(|source| source.as_str())
        .map(|source| source.to_string())
        .expect(
            format!(
                "Error in JSON response from CROM while querying a fragment {}",
                fragment.get("url").unwrap()
            )
            .as_str(),
        )
}

fn _build_crom_query(
    site: &String,
    filter: &Option<String>,
    author: &Option<&String>,
    requested_data: &String,
    after: &Option<&str>,
) -> String {
    let query_body = format!(
        "edges {{
          node {{ {requested_data} }}
        }},
        pageInfo {{
          endCursor,
          hasNextPage
        }}"
    );
    let wikidot_info_filter = match filter {
        Some(filter) => format!("wikidotInfo: {filter},"),
        None => "".to_string(),
    };
    let after_query = match after {
        Some(after) => format!("after: \"{after}\","),
        None => "".to_string(),
    };

    match *author {
        None => format!(
            "query {{
                pages( {after_query} filter: {{ {wikidot_info_filter} url:{{startsWith:\"{site}\"}}}}) {{
                    {query_body}
                }}
            }}"
        ),
        Some(author) => format!(
            "query {{\
                user(name: \"{author}\") {{
                    attributedPages( {after_query} filter: {{ {wikidot_info_filter} url: {{startsWith: \"{site}\"}} }}) {{
                        {query_body}
                    }}
                }}
            }}"
        )
    }
}

async fn _open_browser() -> (Browser, JoinHandle<()>) {
    let (browser, mut handler) = Browser::launch(BrowserConfig::builder().build().expect("Failed to build a Browser to get the files"))
        .await.expect("Failed to launch a Browser to get the files");
    let handler = tokio::task::spawn(async move {
        while let Some(h) = handler.next().await {
            if h.is_err() {
                break;
            }
        }
    });
    let mut username = String::new();
    print!("Wikidot username: ");
    io::stdout().flush().unwrap();
    io::stdin().read_line(&mut username).expect("Failed to read the username.");
    username = username.trim().to_string();
    print!("Password:");
    io::stdout().flush().unwrap();
    let password = read_password().expect("Failed to read the password.").trim().to_string();

    let page = browser.new_page("https://www.wikidot.com/default--flow/login__LoginPopupScreen").await
        .expect("Can't connect to wikidot login page.");
    let fields = page.find_elements("input").await.unwrap();

    fields[0].click().await.unwrap().type_str(username).await.unwrap();
    fields[1].click().await.unwrap().type_str(password).await.unwrap();
    page.find_element("button").await.unwrap().click().await.unwrap();
    page.wait_for_navigation().await.unwrap();
    (browser, handler)
}

async fn _close_browser((browser, handler): (Browser, JoinHandle<()>)) {
    browser.clear_cookies().await
        .inspect_err(|e| {eprintln!("[WARNING] Browser cookies clearing failed: {e}"); }).unwrap_or_default();
    browser.close().await
        .inspect_err(|e| {eprintln!("[WARNING] Failed to close the browser: {e}");}).unwrap_or_default();
    handler.await.unwrap_or_default();
}

pub async fn pages(
    verbose: bool,
    site: &String,
    filter: Option<String>,
    author: Option<&String>,
    requested_data: String,
    gather_fragments_sources: bool,
    download_content: bool,
    get_files: bool,
) -> Vec<Value> {

    let browser_handler = if get_files { Some(_open_browser().await) } else { None };

    let result = _crom_pages(
        verbose,
        site,
        filter,
        author,
        requested_data,
        gather_fragments_sources,
        download_content,
        get_files,
        browser_handler.as_ref().map(|(browser, _)| browser),
        None,
    )
    .await;

    if let Some(browser_handler) = browser_handler {
        _close_browser(browser_handler).await;
    }

    result
}

pub fn xml_escape(s: &str) -> String {
    s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
        .replace("'", "&apos;")
}

pub async fn download_html(
    client: &reqwest::Client,
    url: &str,
    max_retries: i32,
) -> Result<Html, reqwest::Error> {
    let mut retries = 0;
    loop {
        let response = client
            .get(url)
            .header(USER_AGENT, "ScpScriptAnthology/1.0")
            .send()
            .then(async |r| match r {
                Ok(r) => r.text().await,
                Err(e) => Err(e),
            })
            .await;
        if let Err(e) = response {
            if retries < max_retries {
                eprintln!("Request error: {e}. Retrying in 2 seconds.");
                //dbg!(e);
                retries += 1;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            } else {
                break Err(e);
            }
        }
        break Ok(Html::parse_document(response?.as_str()));
    }
}

pub fn write_out<T: Serialize>(script_data: Cli, result: &Vec<T>) {
    match script_data.output_format {
        OutputFormat::JSON => {
            serde_json::to_writer_pretty(script_data.output, &result)
                .expect("Error writing into output file");
        }
        OutputFormat::YAML => {
            serde_yaml::to_writer(script_data.output, &result)
                .expect("Error writing into output file");
        }
    }
}
