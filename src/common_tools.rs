use futures_util::future::join_all;
use futures_util::FutureExt;
use reqwest::header::USER_AGENT;
use scraper::{Html, Selector};
use serde::Serialize;
use serde_json::Value;
use crate::cli::{Cli, OutputFormat};

async fn wait_for_ratelimit(client: &reqwest::Client, crom_url: &str) {
    let mut retries = 0;
    let rate_limit_request = "query {rateLimit{remaining, resetAt}}";
    loop {
        let response = client.post(crom_url)
            .header(USER_AGENT, "ScpScriptAnthology/1.0")
            .json(&serde_json::json!({"query": rate_limit_request}))
            .send().await;
        match response {
            Err(e) => if retries < 5 {
                eprintln!("Request error: {e}. Retrying in 10 seconds.");
                retries += 1;
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            } else {panic!("Too many failed attempts: giving up.")},
            Ok(response) => {
                let json_res: Value = response.json().await.unwrap_or_else(|e| panic!("Recieved data is not JSON? Error: {e}"));
                let remaining = json_res.get("data")
                    .and_then(|data| data.get("rateLimit")
                        .and_then(|ratelimit| ratelimit.get("remaining")
                            .and_then(|remaining| remaining.as_u64())));
                match remaining {
                    Some(0) => {
                        println!("Rate limited by Crom. Waiting 5 minutes.");
                        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                    },
                    None => match json_res.get("errors") {
                        Some(errors) => {
                            eprintln!("Warning: Crom might be flooded! Waiting 30 seconds.\n{errors}");
                            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                            eprintln!("Retrying.");
                        },
                        None => panic!("Error in the JSON response from CROM: {}", json_res.to_string()),
                    }
                    _ => break // Not rate limited
                }
            }
        }
    }
}

pub async fn query_crom(request: &String) -> Value {
    let crom_url = "https://api.crom.avn.sh/graphql";
    let mut retries = 0;
    let client = reqwest::Client::new();
    wait_for_ratelimit(&client, &crom_url).await;

    loop {
        let res: Value = loop {
            assert!(retries < 5, "Too many failed attemps: giving up.");
            let response = client.post(crom_url)
                .header(USER_AGENT, "ScpScriptAnthology/1.0")
                .json(&serde_json::json!({"query": request}))
                .send().await;
            match response {
                Err(e) => {
                    eprintln!("Request error: {e}. Retrying in 10 seconds.");
                    retries += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                },
                Ok(response) => break response.json().await.unwrap_or_else(|e| panic!("Recieved data is not JSON? Error: {e}"))
            }
        };
        match res.get("errors") {
            Some(errors) => {
                eprintln!("Crom returned error(s): {errors}. Waiting 30 seconds.");
                retries += 1;
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
            None => break res
        }
    }
}

async fn _download_webpage(url: &String) -> Option<String> {
    /* Downloading html */
    let mut retries = 0;
    let client = reqwest::Client::new();
    loop {
        if retries > 5 {
            eprintln!("Error while downloading {url}: 5 failed attempts. Giving up.");
            break None;
        }
        let response = client.get(url)
            .header(USER_AGENT, "ScpScriptAnthology/1.0")
            .send().await;
        let html = match response {
            Err(e) => {
                eprintln!("Content downlaod error: {e}. Retrying in 5 seconds.");
                retries += 1;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue
            },
            Ok(response) => match response.text().await {
                Err(e) => {
                    eprintln!("Content format error: {e}. Retrying in 5 seconds.");
                    retries += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue
                },
                Ok(text) => text
            }
        };
        /* Extracting content */
        let page_content_sel = Selector::parse("#page-content").unwrap();
        let doc = Html::parse_document(html.as_str());
        let doc = doc.select(&page_content_sel).next();
        if doc.is_none() {
            eprintln!("No #page-content found for {url}.");
            retries += 1;
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue
        }

        let doc = doc.unwrap();

        let deletion_selectors = vec![
            Selector::parse(".creditRate"),
            Selector::parse(".code"),
            Selector::parse(".footer-wikiwalk-nav")
        ];

        break Some(
            Html::parse_fragment(
                deletion_selectors.into_iter().fold(doc.html(), |collector, selector| {
                    doc.select(&selector.unwrap()).fold(collector, |collector, element| {
                        collector.replace(&element.html(), "")
                    })
                }).as_str()).root_element().text().collect()
        )
    }
}

fn list_children(page: &Value) -> Vec<&Value> {
    page.get("wikidotInfo")
        .and_then(|wikidotinfo| wikidotinfo.get("children"))
        .and_then(|children| children.as_array())
        .map(|children|
            children.into_iter().filter(|child|
                child.get("url")
                    .and_then(|url| url.as_str())
                    .is_some_and(|url| url.contains("fragment:"))
            ).rev().collect()
        ).unwrap_or(Vec::new())
}

async fn crom_pages(verbose: bool, site: &String, filter: Option<String>, author: Option<&String>, requested_data: String, gather_fragments_sources: bool, download_content: bool, after: Option<&str>) -> Vec<Value> {
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
        ).unwrap_or_else(|| panic!("Error in JSON response from CROM: {}\nQuery: {query}", response));

    let mut pages: Vec<Value> = response_parsed.get("edges")
        .and_then(|edges| edges.as_array())
        .expect(format!("Error in JSON response from CROM: {}", response_parsed).as_str())
        .iter().map(|edge| edge.get("node").expect(format!("Error in JSON response from CROM: {}", edge).as_str()).clone())
        .inspect(|page| println!("{}", page.get("url").and_then(|url| url.as_str()).unwrap_or("Invalid URL")))
        .collect();

    if gather_fragments_sources || download_content {
        join_all(pages.iter_mut().map(|page| _add_new_data(verbose, gather_fragments_sources, download_content, page))).await;
    }

    let page_info = response_parsed.get("pageInfo").unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", response_parsed));

    let has_next_page = page_info.get("hasNextPage")
        .and_then(|has_next_page| has_next_page.as_bool())
        .unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", page_info));

    if has_next_page {
        let next_page = page_info.get("endCursor")
            .and_then(|end_cursor| end_cursor.as_str())
            .unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", page_info));
        pages.into_iter().chain(Box::pin(crom_pages(verbose, site, filter, author, requested_data, gather_fragments_sources, download_content, Some(next_page))).await).collect()
    } else {
        pages
    }
}

async fn _add_new_data(verbose: bool, gather_fragments_sources: bool, download_content: bool, page: &mut Value) {
    assert!(page.is_object(), "Error in JSON response from CROM (not an object): {}", page);
    let children: Vec<_> = if gather_fragments_sources { list_children(page) } else { Vec::new() };
    let mut newsource = None;
    if gather_fragments_sources && page.get("wikidotInfo").and_then(|wikidot_info| wikidot_info.get("source")).is_some(){
            newsource = join_all(children.iter().map(|fragment| _gather_fragment_source(verbose, fragment))).await
                .into_iter().reduce(|collector, part| collector + "\n" + part.as_str());
    }

    if download_content {
        if let Some(newcontent) = _download_content(page, children).await {
            page.as_object_mut().unwrap().insert("content".to_string(), Value::String(newcontent));
        } else {
            eprintln!("Warning: no content available for page: {}", page);
        }
    }

    /* Done last to avoid creating a mutable reference to the page before
       now possible because the "children" reference won't be used anymore */
    if let Some(newsource) = newsource {
        if let Some(oldsource) = page.get_mut("wikidotInfo")
            .and_then(|wikidot_info| wikidot_info.as_object_mut())
            .and_then(|wikidot_info| wikidot_info.get_mut("source")) {
            *oldsource = Value::String(newsource.to_string());
        }
    }
}

async fn _download_content(page: &Value, children: Vec<&Value>) -> Option<String> {
    let title = page.get("wikidotInfo").and_then(|wikidotinfo| wikidotinfo.get("title"));
    if let Some(title) = title {
        println!("Downloading content of {title}");
    }

    if !children.is_empty() {
        let newcontent = join_all(children.iter().map(async |fragment|
            _download_webpage(&fragment.get("url").unwrap().as_str().unwrap().to_string()).await
        )).await;
        if newcontent.iter().any(|frag| frag.is_none()) {
            eprintln!("Warning: error when retrieving content for page: {}", page);
        }
        newcontent.into_iter().filter_map(|x| x)
            .reduce(|collector, part| collector + part.as_str())
    } else {
        _download_webpage(&page.get("url").unwrap().as_str().unwrap().to_string()).await
    }
}

async fn _gather_fragment_source(verbose: bool, fragment: &&Value) -> String {
    let query = &format!("
                        query {{
                            page(url:{}){{
                                wikidotInfo {{ source }}
                            }}
                        }}
                   ", fragment.get("url").unwrap());
    if verbose {
        println!("Query: {query}");
    }
    let response = query_crom(query).await;
    if verbose {
        println!("Response: {response}");
    }
    response.get("data")
        .and_then(|d| d.get("page"))
        .and_then(|p| p.get("wikidotInfo"))
        .and_then(|wi| wi.get("source"))
        .and_then(|source| source.as_str())
        .map(|source| source.to_string())
        .unwrap_or_else(|| panic!("Error in JSON response from CROM while querying a fragment {}", fragment.get("url").unwrap()))
}

fn _build_crom_query(site: &String, filter: &Option<String>, author: &Option<&String>, requested_data: &String, after: &Option<&str>) -> String {
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
        None => "".to_string()
    };
    let after_query = match after {
        Some(after) => format!("after: \"{after}\","),
        None => "".to_string()
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

pub async fn pages(verbose: bool, site: &String, filter: Option<String>, author: Option<&String>, requested_data: String, gather_fragments_sources: bool, download_content: bool) -> Vec<Value> {
    crom_pages(verbose, site, filter, author, requested_data, gather_fragments_sources, download_content, None).await
}

pub fn xml_escape(s: &str) -> String {
    s.replace("&", "&amp;").replace("<", "&lt;").replace(">","&gt;").replace('"', "&quot;").replace("'","&apos;")
}

pub async fn download_html(client: &reqwest::Client, url: &str, max_retries: i32) -> Result<Html, reqwest::Error> {
    let mut retries = 0;
    loop {
        let response = client.get(url)
            .header(USER_AGENT, "ScpScriptAnthology/1.0")
            .send().then(async |r| match r {
                Ok(r) => r.text().await,
                Err(e) => Err(e),
            }).await;
        if let Err(e) = response {
            if retries < max_retries {
                eprintln!("Request error: {e}. Retrying in 2 seconds.");
                //dbg!(e);
                retries += 1;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            } else {break Err(e)}
        }
        break Ok(Html::parse_document(response?.as_str()));
    }
}

pub fn write_out<T: Serialize>(script_data: Cli, result: &Vec<T>) {
    match script_data.output_format {
        OutputFormat::JSON => {
            serde_json::to_writer_pretty(script_data.output, &result)
                .unwrap_or_else(|e| panic!("Error writing into output file: {e}"));
        }
        OutputFormat::YAML => {
            serde_yaml::to_writer(script_data.output, &result)
                .unwrap_or_else(|e| panic!("Error writing into output file: {e}"));
        }
    }
}