use reqwest::Client;
use scraper::{Html, Selector};
use serde::Serialize;
use std::sync::OnceLock;
use std::time::Duration;

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

fn client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(8))
            .user_agent("Mozilla/5.0 (compatible; AITokenMonitor/1.0)")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap_or_default()
    })
}

#[derive(Debug, Serialize)]
pub struct UrlMetadata {
    pub url: String,
    pub title: Option<String>,
    pub favicon_url: Option<String>,
}

fn resolve_url(base: &str, href: &str) -> Option<String> {
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_string());
    }
    if href.starts_with("//") {
        return Some(format!("https:{}", href));
    }
    let base_url = reqwest::Url::parse(base).ok()?;
    base_url.join(href).ok().map(|u| u.to_string())
}

fn extract_origin(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    Some(format!("{}://{}", parsed.scheme(), parsed.host_str()?))
}

#[tauri::command]
pub async fn fetch_url_metadata(url: String) -> Result<UrlMetadata, String> {
    // Validate URL
    let parsed = reqwest::Url::parse(&url).map_err(|_| "Invalid URL".to_string())?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err("Only HTTP/HTTPS URLs are supported".to_string());
    }

    let response = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch URL: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let final_url = response.url().to_string();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    let document = Html::parse_document(&body);

    // Extract og:title or <title>
    let og_title_sel = Selector::parse(r#"meta[property="og:title"]"#).unwrap();
    let title_sel = Selector::parse("title").unwrap();

    let title = document
        .select(&og_title_sel)
        .next()
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.trim().to_string())
        .or_else(|| {
            document
                .select(&title_sel)
                .next()
                .map(|el| el.text().collect::<String>().trim().to_string())
        })
        .filter(|s| !s.is_empty());

    // Extract favicon
    let icon_sel = Selector::parse(r#"link[rel~="icon"]"#).unwrap();
    let shortcut_sel = Selector::parse(r#"link[rel="shortcut icon"]"#).unwrap();

    let favicon_href = document
        .select(&icon_sel)
        .chain(document.select(&shortcut_sel))
        .next()
        .and_then(|el| el.value().attr("href"))
        .map(|s| s.to_string());

    let favicon_url = if let Some(href) = favicon_href {
        resolve_url(&final_url, &href)
    } else {
        extract_origin(&final_url).map(|origin| format!("{}/favicon.ico", origin))
    };

    Ok(UrlMetadata {
        url: final_url,
        title,
        favicon_url,
    })
}
