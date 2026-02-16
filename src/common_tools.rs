use crate::cli::{Cli, OutputFormat};
use chromiumoxide::browser::HeadlessMode;
use chromiumoxide::{Browser, BrowserConfig};
use futures_util::future::{join_all, try_join_all, JoinAll, TryJoinAll};
use futures_util::{FutureExt, StreamExt, TryFuture};
use reqwest::header::USER_AGENT;
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use std::error::Error;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_stream::Iter;


/// Exctracts the main content of a Wikidot webpage
pub fn parse_content(doc: &Html) -> Option<String> {
    let page_content_sel = Selector::parse("#page-content").unwrap();
    let Some(doc) = doc.select(&page_content_sel).next() else {
        eprintln!("#page-content not found.");
        return None;
    };

    let deletion_selectors = [
        Selector::parse(".creditRate"),
        Selector::parse(".code"),
        Selector::parse(".footer-wikiwalk-nav"),
    ].into_iter().map(Result::unwrap);

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

#[derive(Debug, Serialize)]
pub struct File {
    pub name: String,
    pub file_type: String,
    pub size: i32
}

impl File {
    pub fn parse(line: ElementRef) -> Self {
        let mut cells = line.children().filter_map(ElementRef::wrap);
        const _GET_STR: fn(Option<ElementRef>) -> String = |cell|
            cell
                .and_then(|cell| cell.children().filter_map(ElementRef::wrap).next())
                .map(|cell| cell.inner_html())
                .unwrap_or(String::new());
        Self {
            name : _GET_STR(cells.next()),
            file_type : _GET_STR(cells.next()),
            size: cells.next().as_ref()
                .map(ElementRef::inner_html)
                .as_deref()
                .map(str::trim)
                .map(_parse_file_size)
                .unwrap_or(0.).round() as i32,
        }
    }
}

fn _parse_file_size(str: &str) -> f32 {
    let Some((n, unit)) = str.split_once(" ") else {
        eprintln!("Can't split file size.");
        return 0.;
    };

    let Ok(n) = n.parse::<f32>() else {
        eprintln!("Can't parse size: {str}.");
        return 0.;
    };

    n * match unit {
        "kB" => 1000.,
        "MB" => 10000000.,
        "Bytes" => 1.,
        u => {
            eprintln!("Unknown unit {u}.");
            1.
        }
    }
}

pub fn file_list(doc: &Html) -> Box<[File]> {
    let file_list_selector = Selector::parse("table.page-files tbody").unwrap();
    let Some(file_list) = doc.select(&file_list_selector).next() else {
        return Box::new([]); // No files
    };

    file_list.children().filter_map(ElementRef::wrap).map(File::parse).collect()
}

/// Downloads a singular webpage.
pub(crate) async fn download_webpage(url: &str) -> Option<String> {
    /* Downloading html */
    let client = reqwest::Client::new();

    retry_async(5, Some(Duration::from_secs(5)), async || {
        let response = client
            .get(url)
            .header(USER_AGENT, "ScpScriptAnthology/1.0")
            .send()
            .await
            .inspect_err(|e| eprintln!("Download error: {e}. Retrying in 5 seconds."))?;

        response.text().await
            .inspect_err(|e| eprintln!("Download error: {e}. Retrying in 5 seconds."))
    }).await.inspect_err(|_| eprintln!("Too many failures, giving up.")).ok()
}

pub async fn download_webpage_browser(url: &str, browser: &Browser) -> Option<String> {
    // Put it in a closure so I can use the ? macro for readability.
    let f = async || {
        let page = browser.new_page(url).await?;
        page.evaluate("WIKIDOT.page.listeners.filesClick();").await?;
        let mut youre_taking_too_long = 30;
        loop {
            youre_taking_too_long -= 1;
            tokio::time::sleep(Duration::from_millis(50)).await;
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

        Ok::<_, Box<dyn Error>>(html)
    };

    f().await.inspect_err(|e| eprintln!("Warning: couldn't download with browser page. Cause: {e}.")).ok()
}



pub async fn open_browser(headless: bool) -> (Browser, JoinHandle<()>) {
    let (browser, mut handler) = Browser::launch(
        BrowserConfig::builder()
            .headless_mode(if headless {HeadlessMode::True} else {HeadlessMode::False})
            .build()
            .expect("Failed to build a Browser to get the files")
    )
        .await.expect("Failed to launch a Browser to get the files");
    let handler = tokio::task::spawn(async move {
        while let Some(h) = handler.next().await {
            if h.is_err() {
                break;
            }
        }
    });
    (browser, handler)
}

pub async fn close_browser((browser, handle): (Browser, JoinHandle<()>)) {
    browser.clear_cookies().await
        .inspect_err(|e| {eprintln!("[WARNING] Browser cookies clearing failed: {e}"); }).unwrap_or_default();
    browser.close().await
        .inspect_err(|e| {eprintln!("[WARNING] Failed to close the browser: {e}");}).unwrap_or_default();
    handle.await.unwrap_or_default();
}



pub fn xml_escape(s: &str) -> String {
    const ESC: [(&str, &str); 5] = [
        ("&", "&amp;"),
        ("<", "&lt;"),
        (">", "&gt;"),
        ("\"", "&quot;"),
        ("'", "&apos;"),
    ];
    ESC.into_iter().fold(s.to_string(), |acc, (source, cible)| acc.replace(source, cible))
}

pub async fn download_html(
    client: &reqwest::Client,
    url: &str,
    max_retries: usize,
) -> Result<Html, reqwest::Error> {
    retry_async(max_retries, Some(Duration::from_secs(2)), async || {
        client
            .get(url)
            .header(USER_AGENT, "ScpScriptsAnthology/1.0")
            .send()
            .then(async |r| match r {
                Ok(r) => r.text().await,
                Err(e) => Err(e),
            })
            .await
            .inspect_err(|e| eprintln!("Request error: {e}. Retrying in 2 seconds."))
    }).await
        .map(|s| Html::parse_document(s.as_str()))
}

pub fn write_out<T: Serialize>(script_data: Cli, result: &[T]) {
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

#[allow(unused)]
pub trait FutureIterator<F: Future>: Sized + Iterator<Item = F> {
    fn into_future_iter(self) -> Iter<Self> {
        tokio_stream::iter(self)
    }

    fn join_all(self) -> JoinAll<F> {
        join_all(self)
    }
}

#[allow(unused)]
pub trait TryFutureIterator<F: TryFuture>: FutureIterator<F> {
    fn try_join_all(self) -> TryJoinAll<F> {
        try_join_all(self)
    }
}

impl<I: Iterator<Item = F>, F: Future> FutureIterator<F> for I {}
impl<I: Iterator<Item = F>, F: TryFuture> TryFutureIterator<F> for I {}

#[allow(unused)]
pub trait TryIterator<R, E>: Sized + Iterator<Item = Result<R, E>> {
    fn stable_try_collect<C: FromIterator<R> + Default>(mut self) -> Result<C, E> {
        let error = self.find(|r| r.is_err());
        error.map(|r| r.map(|_| C::default()))
            .unwrap_or_else(|| self.collect())
    }

    fn partition_errors<C: FromIterator<R>, X: FromIterator<E>>(self) -> (C, X) {
        let (oks, errs): (Vec<_>, Vec<_>) = self.partition(|r| r.is_ok());
        (oks.into_iter().filter_map(Result::ok).collect(), errs.into_iter().filter_map(Result::err).collect())
    }
}

impl<R, E, I: Sized + Iterator<Item = Result<R, E>>> TryIterator<R, E> for I {}

#[allow(unused)]
pub(crate) async fn retry_async<O, E>(mut retries: usize, sleep: Option<Duration>, f: impl AsyncFn() -> Result<O, E>) -> Result<O, E> {
    let mut res = f().await;
    while retries > 0 && res.is_err() {
        if let Some(dur) = sleep {
            tokio::time::sleep(dur).await;
        }
        retries -= 1;
        res = f().await;
    }
    res
}