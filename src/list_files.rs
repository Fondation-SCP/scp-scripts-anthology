use std::collections::HashMap;
use std::sync::Arc;
use clap::Parser;
use futures_util::{FutureExt, StreamExt};
use reqwest::Client;
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;
use crate::cli::{Cli, Script};
use crate::common_tools;
use crate::common_tools::FutureIterator;

#[derive(Parser)]
#[command(version = "0.1.0")]
pub struct ListFilesParameters {
    /// Unix name of the page where the ListPages module listing the pages whose files you want to list is located., value_name = "URL"
    listpages_location: String,
    /// Shows the browser
    #[arg(long, default_value = "false")]
    no_headless: bool,
}

async fn _get_file_list_from_listpage_page(client: Arc<Client>, url: String, selectors: Arc<(Selector, Selector)>) -> Vec<String> {
    let Ok(page) = common_tools::download_html(client.as_ref(), url.as_str(), 5).await else {
        eprintln!("Couldn't download page {url}. Gave up retrying.");
        return vec![];
    };

    let (list_selector, link_selector) = selectors.as_ref();

    page.select(list_selector).next()
        .expect("Page list not found. Have you put the ListPages inside a div with the ssa-list-files class?")
        .select(link_selector)
        .filter_map(|link| link.attr("href"))
        .map(|s| s[1..].to_string())
        .collect()
}

pub async fn list_files(mut script_data: Cli) {
    let Script::ListFiles(params) = &mut script_data.script else {
        panic!("Unreachable code")
    };

    let client = Client::new();
    let site_url = script_data.site.clone().unwrap();
    let listpages_url = site_url.clone() + params.listpages_location.as_str();
    let first_page = common_tools::download_html(&client, listpages_url.as_str(), 5).await
        .expect("Failed to download the page containing the ListPages module.");

    let pager_selector = Selector::parse(".pager").unwrap();
    let page_count = first_page.select(&pager_selector).next()
        .expect("Pager not found on the page where ListPages should be.")
        .children().filter_map(ElementRef::wrap).next()
        .expect("Pager has no element children?")
        .inner_html().split(" ").last()
        .expect("Pager page indicator is empty.")
        .parse::<usize>().expect("Could not parse the number of pages from the pager.");

    let arc_client = Arc::new(client);
    let arc_selectors = Arc::new((
                                     Selector::parse("div.ssa-list-files p").unwrap(),
                                     Selector::parse("a").unwrap()
    ));

    let page_list = (1..=page_count).map(|page_nb| {
        listpages_url.clone() + "/p/" + page_nb.to_string().as_str()
    })
        .map(|page_url| _get_file_list_from_listpage_page(arc_client.clone(), page_url, arc_selectors.clone()))
        .into_future_iter()
        .buffer_unordered(script_data.threads)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    println!("{} pages found.", page_list.len());

    let (browser, handler) = common_tools::open_browser(!params.no_headless).await;
    let arc_browser = Arc::new(browser);


    let pages_html = page_list.into_iter()
        .map(|url| async {
            println!("Downloading {url}");
            let page = common_tools::download_webpage_browser(
                (site_url.clone() + url.as_str()).as_str(),
                arc_browser.clone().as_ref()
            ).await;
            (url, page)
        }.boxed()) /* Boxed because too big for the stack */
        .into_future_iter()
        .buffer_unordered(script_data.threads)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|(url, page_content)| page_content.map(|p| (url, p)))
        .map(|(url, page_content)| (url, Html::parse_document(page_content.as_str())))
        .map(|(url, html)| (url, common_tools::file_list(&html)))
        .filter_map(|(url, files)| match files.as_slice() {
            [] => None,
            _ => {
                let mut page = HashMap::new();
                page.insert("url".to_string(), Value::String(url));
                let page_size = files.iter().fold(0, |total, file| total + file.size);
                page.insert("total size".to_string(), Value::Number(page_size.into()));
                page.insert("files".to_string(), serde_json::to_value(files).unwrap());
                Some(page)
            }
        })
        .collect::<Vec<_>>();     

    common_tools::close_browser((Arc::into_inner(arc_browser).unwrap(), handler)).await;

    let path = script_data.output.path().clone();

    common_tools::write_out(script_data, &pages_html);

    println!("Results written in file {}", path);

}