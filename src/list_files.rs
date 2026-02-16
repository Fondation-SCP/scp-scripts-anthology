use crate::cli::{Cli, Script};
use crate::common_tools;
use crate::common_tools::FutureIterator;
use clap::Parser;
use futures_util::{FutureExt, StreamExt};
use reqwest::Client;
use scraper::{ElementRef, Html, Selector};
use std::sync::Arc;
use lazy_static::lazy_static;

#[derive(Parser)]
#[command(version = "0.1.0")]
pub struct ListFilesParameters {
    /// Unix name of the page where the ListPages module listing the pages whose files you want to list is located., value_name = "URL"
    listpages_location: String,
    /// Shows the browser
    #[arg(long, default_value = "false")]
    no_headless: bool,
}

lazy_static!(
    static ref PAGE_SELECTOR: Selector = Selector::parse(".pager").unwrap();
    static ref LIST_SELECTOR: Selector = Selector::parse("div.ssa-list-files p").unwrap();
    static ref LINK_SELECTOR: Selector = Selector::parse("a").unwrap();
);

async fn _get_file_list_from_listpage_page(client: Arc<Client>, url: String) -> Vec<String> {
    let Ok(page) = common_tools::download_html(client.as_ref(), url.as_str(), 5).await else {
        eprintln!("Couldn't download page {url}. Gave up retrying.");
        return vec![];
    };

    page.select(&LIST_SELECTOR).next()
        .expect("Page list not found. Have you put the ListPages inside a div with the ssa-list-files class?")
        .select(&LINK_SELECTOR)
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

    let page_count = first_page.select(&PAGE_SELECTOR).next()
        .expect("Pager not found on the page where ListPages should be.")
        .children().filter_map(ElementRef::wrap).next()
        .expect("Pager has no element children?")
        .inner_html().split(" ").last()
        .expect("Pager page indicator is empty.")
        .parse::<usize>().expect("Could not parse the number of pages from the pager.");

    let arc_client = Arc::new(client);

    let page_list = (1..=page_count).map(|page_nb| {
        listpages_url.clone() + "/p/" + page_nb.to_string().as_str()
    })
        .map(|page_url| _get_file_list_from_listpage_page(arc_client.clone(), page_url))
        .into_future_iter()
        .buffer_unordered(script_data.threads)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect::<Box<[_]>>();

    println!("{} pages found.", page_list.len());

    let (browser, handler) = common_tools::open_browser(!params.no_headless).await;
    let arc_browser = Arc::new(browser);

    let pages_html = page_list.into_iter()
        .map(|url| async {
            println!("Downloading {url}");
            let page = common_tools::download_webpage_browser(
                (site_url.clone() + url.as_str()).as_str(),
                arc_browser.clone().as_ref()
            ).await.map(Box::new); /* Boxed because too big for the stack */
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
        .filter(|(_, files)| !files.is_empty())
        .map(|(url, files)| {
            [
                ("url", serde_json::to_value(url).unwrap()),
                ("total size", serde_json::to_value(files.iter().map(|file| file.size).sum::<i32>()).unwrap()),
                ("files", serde_json::to_value(files).unwrap())
            ]
        })
        .collect::<Box<[_]>>();

    common_tools::close_browser((Arc::into_inner(arc_browser).unwrap(), handler)).await;

    let path = script_data.output.path().clone();

    common_tools::write_out(script_data, pages_html.as_ref());

    println!("Results written in file {}", path);

}