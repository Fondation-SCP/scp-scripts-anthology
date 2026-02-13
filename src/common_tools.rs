use crate::cli::{Cli, OutputFormat};
use chromiumoxide::browser::HeadlessMode;
use chromiumoxide::{Browser, BrowserConfig};
use futures_util::future::{join_all, try_join_all, JoinAll, TryJoinAll};
use futures_util::{FutureExt, StreamExt, TryFuture};
use reqwest::header::USER_AGENT;
use rpassword::read_password;
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use std::error::Error;
use std::io;
use std::io::Write;
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

#[derive(Debug, Serialize)]
pub struct File {
    pub name: String,
    pub file_type: String,
    pub size: i32
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

pub fn file_list(doc: &Html) -> Vec<File> {
    let file_list_selector = Selector::parse("table.page-files tbody").unwrap();
    let Some(file_list) = doc.select(&file_list_selector).next() else {
        return vec![]; // No files
    };

    file_list.children().filter_map(ElementRef::wrap).map(|line| {
        let cells = line.children().filter_map(ElementRef::wrap).collect::<Vec<_>>();
        File {
            name : cells.first()
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
pub(crate) async fn _download_webpage(url: &str) -> Option<String> {
    /* Downloading html */
    let mut retries = -1;
    let client = reqwest::Client::new();
    loop {
        match retries {
            -1 => {
                retries = 0;
            }
            i if i <= 5 => {
                tokio::time::sleep(Duration::from_secs(5)).await;
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

pub async fn close_browser((browser, handle): (Browser, JoinHandle<()>)) {
    browser.clear_cookies().await
        .inspect_err(|e| {eprintln!("[WARNING] Browser cookies clearing failed: {e}"); }).unwrap_or_default();
    browser.close().await
        .inspect_err(|e| {eprintln!("[WARNING] Failed to close the browser: {e}");}).unwrap_or_default();
    handle.await.unwrap_or_default();
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
            .header(USER_AGENT, "ScpScriptsAnthology/1.0")
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
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            } else {
                break Err(e);
            }
        }
        break Ok(Html::parse_document(response?.as_str()));
    }
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
fn retry<O, E>(retries: usize, f: impl Fn() -> Result<O, E>) -> Result<O, E> {
    let res = f();
    if retries == 0 || res.is_ok() {
        res
    } else {
        retry(retries - 1, f)
    }
}

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