use chrono::{TimeZone, Utc};
use regex::Regex;
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Feed {
    pub title: String,
    pub link: String,
    pub description: String,
    pub items: Vec<FeedItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FeedItem {
    pub title: String,
    pub link: String,
    pub pub_date: Option<String>,
    pub description: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Comment {
    pub author: Option<String>,
    pub date: Option<String>,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Video {
    pub url: String,
    pub thumbnail: Option<String>,
    pub platform: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExtractedContent {
    pub title: String,
    pub text: String,
    pub images: Vec<String>,
    pub videos: Vec<Video>,
    pub byline: Option<String>,
    pub comments: Vec<Comment>,
}

fn create_client() -> Client {
    Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap()
}

#[tauri::command]
fn fetch_feed(url: String) -> Result<Feed, String> {
    let client = create_client();
    
    let response = client.get(&url).send().map_err(|e| e.to_string())?;
    let content = response.text().map_err(|e| e.to_string())?;
    
    if content.contains("<rss") {
        parse_rss(&content)
    } else if content.contains("<feed") {
        parse_atom(&content)
    } else {
        Err("Unknown feed format".to_string())
    }
}

fn parse_rss(content: &str) -> Result<Feed, String> {
    let channel = rss::Channel::read_from(content.as_bytes()).map_err(|e| e.to_string())?;
    
    let items: Vec<FeedItem> = channel.items().iter().map(|item| {
        FeedItem {
            title: item.title().unwrap_or("").to_string(),
            link: item.link().unwrap_or("").to_string(),
            pub_date: item.pub_date().map(|s| s.to_string()),
            description: item.description().map(|s| s.to_string()),
            content: item.content().map(|s| s.to_string()),
        }
    }).collect();
    
    Ok(Feed {
        title: channel.title().to_string(),
        link: channel.link().to_string(),
        description: channel.description().to_string(),
        items,
    })
}

fn parse_atom(content: &str) -> Result<Feed, String> {
    let feed = atom_syndication::Feed::read_from(content.as_bytes()).map_err(|e| e.to_string())?;
    
    let items: Vec<FeedItem> = feed.entries().iter().map(|entry| {
        let link = entry.links().first().map(|l| l.href().to_string()).unwrap_or_default();
        
        let content_val = entry.content().and_then(|c| c.value.as_ref().map(|s| s.to_string()));
        
        FeedItem {
            title: entry.title().to_string(),
            link,
            pub_date: entry.published().map(|s| s.to_string()),
            description: entry.summary().as_ref().map(|s| s.to_string()),
            content: content_val,
        }
    }).collect();
    
    let feed_link = feed.links().first().map(|l| l.href().to_string()).unwrap_or_default();
    let feed_subtitle = feed.subtitle().as_ref().map(|s| s.to_string()).unwrap_or_default();
    
    Ok(Feed {
        title: feed.title().to_string(),
        link: feed_link,
        description: feed_subtitle,
        items,
    })
}

#[tauri::command]
fn extract_content(url: String, filters: Vec<String>) -> Result<ExtractedContent, String> {
    if url.contains("news.ycombinator.com/item") || url.contains("hackernews.com/item") {
        return extract_hackernews(&url, &filters);
    }
    
    let client = create_client();
    
    let response = client.get(&url).send().map_err(|e| e.to_string())?;
    let html = response.text().map_err(|e| e.to_string())?;
    
    extract_article(&html, &filters)
}

fn extract_hackernews(url: &str, filters: &[String]) -> Result<ExtractedContent, String> {
    let client = create_client();
    
    let id = url.split("id=").nth(1).unwrap_or("");
    let id = id.split('&').next().unwrap_or(id);
    
    if id.is_empty() {
        return Err("Could not find Hacker News item ID".to_string());
    }
    
    let item_url = format!("https://hacker-news.firebaseio.com/v0/item/{}.json", id);
    let response = client.get(&item_url).send().map_err(|e: reqwest::Error| e.to_string())?;
    let item: serde_json::Value = response.json().map_err(|e: reqwest::Error| e.to_string())?;
    
    let title = item["title"].as_str().unwrap_or("Hacker News").to_string();
    let text = item["text"].as_str().unwrap_or("").to_string();
    let byline = item["by"].as_str().map(|s: &str| s.to_string());
    
    let mut comments = Vec::new();
    if let Some(kids) = item["kids"].as_array() {
        for kid_id in kids.iter().take(20) {
            if let Some(id) = kid_id.as_i64() {
                if let Ok(comment) = fetch_hn_comment(&client, id, 0, filters) {
                    comments.push(comment);
                }
            }
        }
    }
    
    Ok(ExtractedContent {
        title,
        text,
        images: vec![],
        videos: vec![],
        byline,
        comments,
    })
}

fn fetch_hn_comment(client: &Client, id: i64, depth: usize, filters: &[String]) -> Result<Comment, String> {
    if depth > 3 {
        return Err("Max depth reached".to_string());
    }
    
    let url = format!("https://hacker-news.firebaseio.com/v0/item/{}.json", id);
    let response = client.get(&url).send().map_err(|e: reqwest::Error| e.to_string())?;
    let item: serde_json::Value = response.json().map_err(|e: reqwest::Error| e.to_string())?;
    
    if item["type"] != "comment" {
        return Err("Not a comment".to_string());
    }
    
    let text = item["text"].as_str().unwrap_or("").to_string();
    let author = item["by"].as_str().map(|s: &str| s.to_string());
    let date = item["time"].as_i64().map(|t: i64| {
        let dt = Utc.timestamp_opt(t, 0).unwrap();
        dt.format("%Y-%m-%d %H:%M").to_string()
    });
    
    let mut replies = Vec::new();
    if let Some(kids) = item["kids"].as_array() {
        for kid_id in kids.iter().take(5) {
            if let Some(kid) = kid_id.as_i64() {
                if let Ok(comment) = fetch_hn_comment(client, kid, depth + 1, filters) {
                    replies.push(comment);
                }
            }
        }
    }
    
    let full_text = if replies.is_empty() {
        text.clone()
    } else {
        let mut result = text.clone();
        for reply in &replies {
            result.push_str("\n\n---\n");
            result.push_str(&reply.text);
        }
        result
    };
    
    Ok(Comment {
        author,
        date,
        text: clean_text(&full_text),
    })
}

fn extract_article(html: &str, filters: &[String]) -> Result<ExtractedContent, String> {
    let document = Html::parse_document(&html);
    
    let (title, text, images, videos) = extract_article_content(&document, filters);
    let byline = extract_byline(&document);
    let comments = extract_comments(&document);
    
    Ok(ExtractedContent {
        title,
        text,
        images,
        videos,
        byline,
        comments,
    })
}

fn extract_article_content(document: &Html, filters: &[String]) -> (String, String, Vec<String>, Vec<Video>) {
    let bad_selectors = vec![
        "script", "style", "nav", "header", "footer", "aside",
        "iframe", "noscript", "form", "input", "button"
    ];
    
    let bad_classes = vec![
        "sidebar", "advertisement", "ad", "ads", "comments", "comment",
        "navigation", "menu", "social", "share", "sharing", "social-share",
        "related", "recommended", "popup", "modal", "newsletter", "subscribe",
        "breadcrumb", "tags", "tag", "author-bio", "bio", "meta", "timestamp",
        "vote", "rating", "carousel", "slider", "gallery", "pagination",
        "print", "pdf", "download", "sticky", "fixed", "header", "footer",
        "nav", "menu", "widget", "sidebar", "social", "share", "sharing",
        "facebook", "twitter", "instagram", "linkedin", "youtube", "tiktok",
        "comment", "reply", "author", "bio", "date", "time", "category"
    ];
    
    let content_selectors = vec![
        "article", "[role=main]", "main", ".post-content", ".article-content",
        ".entry-content", ".content", "#content", ".post", ".article", ".story"
    ];
    
    let social_domains = vec![
        "facebook.com", "fbcdn.net", "facebook.net", "twitter.com", "x.com",
        "instagram.com", "linkedin.com", "pinterest.com", "tiktok.com",
        "youtube.com", "youtu.be", "snapchat.com", "reddit.com", "tumblr.com",
        "discord.com", "telegram.org", "whatsapp.com", "messenger", "wechat",
        "line.me", "snap.licdn.com", "platform.twitter", "platform.linkedin",
        "twimg.com", "fb.com", "t.co", "bit.ly", "goo.gl", "fbcdn.com",
        "amazon-adsystem", "doubleclick", "googlesyndication", "adnxs.com"
    ];
    
    let social_classes = vec![
        "social", "share", "sharing", "icon", "favicon", "logo", "avatar",
        "profile", "follow", "like", "button", "widget", "sidebar", "footer",
        "header", "nav", "menu", "comment", "reply", "author", "bio", "meta",
        "breadcrumb", "tags", "category", "date", "time", "newsletter",
        "subscribe", "popup", "modal", "sticky", "fixed", "carousel",
        "slider", "gallery", "pagination", "print", "pdf", "download",
        "advertisement", "ad", "ads", "sponsor", "promo", "tracking",
        "pixel", "analytics", "sharethis", "addthis", "share-button",
        "social-button", "fb-like", "tweet-button", "linkedin-share"
    ];
    
    let selectors_slice = content_selectors.as_slice();
    
    let mut title = String::new();
    let mut content = String::new();
    let mut images = Vec::new();
    let mut seen = std::collections::HashSet::new();
    
    for selector_str in selectors_slice {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                if is_bad_element_extended(&element, &bad_selectors, &bad_classes) {
                    continue;
                }
                
                let element_text = element.text().collect::<String>();
                if element_text.len() > 200 {
                    content = filter_article_text(&element_text, filters);
                    content = clean_text(&content);
                    
                    title = extract_title_from_element(&element);
                    
                    if let Ok(img_selector) = Selector::parse("img") {
                        for img in element.select(&img_selector) {
                            if let Some(src) = img.value().attr("src") {
                                let clean_src = clean_url(src);
                                if is_valid_article_image(&clean_src, &social_domains, &social_classes, &img) {
                                    if !seen.contains(&clean_src) {
                                        seen.insert(clean_src.clone());
                                        images.push(clean_src);
                                    }
                                }
                            }
                            if let Some(src) = img.value().attr("data-src") {
                                let clean_src = clean_url(src);
                                if is_valid_article_image(&clean_src, &social_domains, &social_classes, &img) {
                                    if !seen.contains(&clean_src) {
                                        seen.insert(clean_src.clone());
                                        images.push(clean_src);
                                    }
                                }
                            }
                        }
                    }
                    
                    if let Ok(figure_selector) = Selector::parse("figure img") {
                        for img in element.select(&figure_selector) {
                            if let Some(src) = img.value().attr("src") {
                                let clean_src = clean_url(src);
                                if is_valid_article_image(&clean_src, &social_domains, &social_classes, &img) {
                                    if !seen.contains(&clean_src) {
                                        seen.insert(clean_src.clone());
                                        images.push(clean_src);
                                    }
                                }
                            }
                        }
                    }
                    
                    if let Ok(picture_selector) = Selector::parse("picture source") {
                        for source in element.select(&picture_selector) {
                            if let Some(srcset) = source.value().attr("srcset") {
                                for part in srcset.split(',') {
                                    let src = part.trim().split_whitespace().next().unwrap_or("");
                                    let clean_src = clean_url(src);
                                    if !clean_src.is_empty() && !seen.contains(&clean_src) {
                                        seen.insert(clean_src.clone());
                                        images.push(clean_src);
                                    }
                                }
                            }
                        }
                    }
                    
                    break;
                }
            }
        }
    }
    
    if content.is_empty() {
        if let Ok(selector) = Selector::parse("body") {
            if let Some(element) = document.select(&selector).next() {
                content = element.text().collect::<String>();
                content = filter_article_text(&content, filters);
                content = clean_text(&content);
            }
        }
    }
    
    if title.is_empty() {
        title = extract_title(document);
    }
    
    let filtered_content = filter_article_text(&content, filters);
    
    let videos = extract_videos(document, selectors_slice);
    
    (title, filtered_content, images, videos)
}

fn is_bad_element_extended(elem: &scraper::ElementRef, bad: &[&str], bad_classes: &[&str]) -> bool {
    let tag_name = elem.value().name();
    
    for sel in bad {
        if tag_name == *sel {
            return true;
        }
    }
    
    if let Some(class) = elem.value().attr("class") {
        let class_lower = class.to_lowercase();
        for sel in bad {
            if class_lower.contains(sel) {
                return true;
            }
        }
        for sel in bad_classes {
            if class_lower.contains(sel) {
                return true;
            }
        }
    }
    
    if let Some(id) = elem.value().attr("id") {
        let id_lower = id.to_lowercase();
        for sel in bad {
            if id_lower.contains(sel) {
                return true;
            }
        }
        for sel in bad_classes {
            if id_lower.contains(sel) {
                return true;
            }
        }
    }
    
    false
}

fn extract_title(document: &Html) -> String {
    let selectors = ["h1", "article h1", ".article-title", ".post-title", ".entry-title"];
    
    for selector_str in selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                let text = element.text().collect::<String>().trim().to_string();
                if !text.is_empty() {
                    return text;
                }
            }
        }
    }
    
    if let Ok(selector) = Selector::parse("title") {
        if let Some(element) = document.select(&selector).next() {
            return element.text().collect::<String>().trim().to_string();
        }
    }
    
    String::new()
}

fn extract_title_from_element(element: &scraper::ElementRef) -> String {
    let selectors = ["h1", "h2", ".title", ".post-title", ".entry-title", ".article-title"];
    
    for selector_str in selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(el) = element.select(&selector).next() {
                let text = el.text().collect::<String>().trim().to_string();
                if !text.is_empty() && text.len() < 200 {
                    return text;
                }
            }
        }
    }
    
    String::new()
}

fn is_valid_article_image(url: &str, social_domains: &[&str], social_classes: &[&str], img: &scraper::ElementRef) -> bool {
    if url.is_empty() {
        return false;
    }
    
    let url_lower = url.to_lowercase();
    
    for domain in social_domains {
        if url_lower.contains(domain) {
            return false;
        }
    }
    
    let icon_patterns = ["icon", "button", "share", "social", "follow", "like", "tweet", "pin", "avatar", "profile", "logo", "favicon", "symbol", "badge", "sprite", "small", "thumb"];
    for pattern in icon_patterns {
        if url_lower.contains(pattern) && (url_lower.contains(".png") || url_lower.contains(".svg") || url_lower.contains(".gif") || url_lower.contains(".ico") || url_lower.contains(".jpg") || url_lower.contains(".jpeg")) {
            return false;
        }
    }
    
    if url_lower.ends_with(".ico") || url_lower.contains("favicon") {
        return false;
    }
    
    if let Some(class) = img.value().attr("class") {
        let class_lower = class.to_lowercase();
        for sel in social_classes {
            if class_lower.contains(sel) {
                return false;
            }
        }
    }
    
    if let Some(id) = img.value().attr("id") {
        let id_lower = id.to_lowercase();
        for sel in social_classes {
            if id_lower.contains(sel) {
                return false;
            }
        }
    }
    
    if let Some(alt) = img.value().attr("alt") {
        let alt_lower = alt.to_lowercase();
        let social_alt_patterns = ["share", "facebook", "twitter", "instagram", "linkedin", "pinterest", "youtube", "tiktok", "follow", "like", "tweet", "connect"];
        for pattern in social_alt_patterns {
            if alt_lower.contains(pattern) {
                return false;
            }
        }
    }
    
    if let Some(width) = img.value().attr("width") {
        if let Ok(w) = width.parse::<u32>() {
            if w < 100 {
                return false;
            }
        }
    }
    
    if let Some(height) = img.value().attr("height") {
        if let Ok(h) = height.parse::<u32>() {
            if h < 50 {
                return false;
            }
        }
    }
    
    if let Some(style) = img.value().attr("style") {
        let style_lower = style.to_lowercase();
        if style_lower.contains("width") {
            if let Some(px_pos) = style_lower.find("px") {
                let start = style_lower[..px_pos].rfind(|c| c == ' ' || c == ':').map(|p| p + 1).unwrap_or(0);
                let val_str = &style_lower[start..px_pos];
                if let Ok(w) = val_str.trim().parse::<u32>() {
                    if w < 100 {
                        return false;
                    }
                }
            }
        }
    }
    
    true
}

fn extract_byline(document: &Html) -> Option<String> {
    let selectors = [".author", ".byline", "[rel=author]", ".post-author", ".article-author"];
    
    for selector_str in selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                let text = element.text().collect::<String>().trim().to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
    }
    
    None
}

fn extract_comments(document: &Html) -> Vec<Comment> {
    let mut comments = Vec::new();
    
    let comment_selectors = vec![
        ".comment", ".comments", ".comment-list", ".commentlist",
        ".comments-list", ".response", ".responses", ".replies",
        "#comments", "#commentlist", "[id*='comment']", ".user-comments"
    ];
    
    for selector_str in comment_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(section) = document.select(&selector).next() {
                if let Ok(comment_selector) = Selector::parse(".comment-body, .comment, .comment-item, .user-comment") {
                    for comment_elem in section.select(&comment_selector) {
                        let author = extract_comment_author(&comment_elem);
                        let date = extract_comment_date(&comment_elem);
                        let text = extract_comment_text(&comment_elem);
                        
                        if !text.trim().is_empty() {
                            comments.push(Comment {
                                author,
                                date,
                                text: clean_text(&text),
                            });
                        }
                    }
                }
                
                if comments.is_empty() {
                    if let Ok(p_selector) = Selector::parse("p") {
                        for para in section.select(&p_selector) {
                            let text = para.text().collect::<String>().trim().to_string();
                            if text.len() > 20 && !text.to_lowercase().contains("comment") {
                                comments.push(Comment {
                                    author: None,
                                    date: None,
                                    text: clean_text(&text),
                                });
                            }
                        }
                    }
                }
                
                if !comments.is_empty() {
                    break;
                }
            }
        }
    }
    
    comments.truncate(20);
    comments
}

fn extract_comment_author(elem: &scraper::ElementRef) -> Option<String> {
    let selectors = [".author", ".comment-author", ".user-name", ".username", "[rel='author']", ".fn", ".comment__author"];
    
    for selector_str in selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(el) = elem.select(&selector).next() {
                let text = el.text().collect::<String>().trim().to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
    }
    
    if let Ok(selector) = Selector::parse("a") {
        if let Some(el) = elem.select(&selector).next() {
            let text = el.text().collect::<String>().trim().to_string();
            if !text.is_empty() && text.len() < 50 {
                return Some(text);
            }
        }
    }
    
    None
}

fn extract_comment_date(elem: &scraper::ElementRef) -> Option<String> {
    let selectors = [".date", ".comment-date", ".timestamp", ".time", ".comment-time", "time[datetime]"];
    
    for selector_str in selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(el) = elem.select(&selector).next() {
                let text = el.text().collect::<String>().trim().to_string();
                if !text.is_empty() {
                    return Some(text);
                }
                if let Some(datetime) = el.value().attr("datetime") {
                    return Some(datetime.to_string());
                }
            }
        }
    }
    
    None
}

fn extract_comment_text(elem: &scraper::ElementRef) -> String {
    elem.text().collect::<String>()
}

fn extract_videos(document: &Html, content_selectors: &[&str]) -> Vec<Video> {
    let mut videos = Vec::new();
    let mut seen = std::collections::HashSet::new();
    
    for selector_str in content_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                if let Ok(iframe_selector) = Selector::parse("iframe[src]") {
                    for iframe in element.select(&iframe_selector) {
                        if let Some(src) = iframe.value().attr("src") {
                            if let Some(video) = extract_video_info(&src) {
                                if !seen.contains(&video.url) {
                                    seen.insert(video.url.clone());
                                    videos.push(video);
                                }
                            }
                        }
                        if let Some(src) = iframe.value().attr("data-src") {
                            if let Some(video) = extract_video_info(&src) {
                                if !seen.contains(&video.url) {
                                    seen.insert(video.url.clone());
                                    videos.push(video);
                                }
                            }
                        }
                    }
                }
                
                if let Ok(video_selector) = Selector::parse("video source[src]") {
                    for source in element.select(&video_selector) {
                        if let Some(src) = source.value().attr("src") {
                            let clean_src = clean_url(src);
                            if !clean_src.is_empty() && !seen.contains(&clean_src) {
                                seen.insert(clean_src.clone());
                                videos.push(Video {
                                    url: clean_src,
                                    thumbnail: None,
                                    platform: Some("HTML5".to_string()),
                                });
                            }
                        }
                    }
                }
                
                if let Ok(video_tag) = Selector::parse("video[src]") {
                    for video in element.select(&video_tag) {
                        if let Some(src) = video.value().attr("src") {
                            let clean_src = clean_url(src);
                            if !clean_src.is_empty() && !seen.contains(&clean_src) {
                                seen.insert(clean_src.clone());
                                videos.push(Video {
                                    url: clean_src,
                                    thumbnail: None,
                                    platform: Some("HTML5".to_string()),
                                });
                            }
                        }
                    }
                }
                
                if let Ok(source_tag) = Selector::parse("source[type^='video']") {
                    for source in element.select(&source_tag) {
                        if let Some(src) = source.value().attr("src") {
                            let clean_src = clean_url(src);
                            if !clean_src.is_empty() && !seen.contains(&clean_src) {
                                seen.insert(clean_src.clone());
                                videos.push(Video {
                                    url: clean_src,
                                    thumbnail: None,
                                    platform: Some("HTML5".to_string()),
                                });
                            }
                        }
                    }
                }
                
                break;
            }
        }
    }
    
    videos.truncate(5);
    videos
}

fn extract_video_info(url: &str) -> Option<Video> {
    let url_lower = url.to_lowercase();
    
    if url_lower.contains("youtube.com") || 
       url_lower.contains("youtu.be") ||
       url_lower.contains("youtube-nocookie.com") ||
       url_lower.contains("ytimg.com") {
        if let Some(video_id) = extract_youtube_id(&url_lower) {
            let thumbnail = Some(format!("https://img.youtube.com/vi/{}/maxresdefault.jpg", video_id));
            return Some(Video {
                url: url.to_string(),
                thumbnail,
                platform: Some("YouTube".to_string()),
            });
        }
        
        if url_lower.contains("youtube.com/embed/") || url_lower.contains("youtube-nocookie.com/embed/") {
            return Some(Video {
                url: url.to_string(),
                thumbnail: None,
                platform: Some("YouTube".to_string()),
            });
        }
    }
    
    if url_lower.contains("vimeo.com") {
        return Some(Video {
            url: url.to_string(),
            thumbnail: None,
            platform: Some("Vimeo".to_string()),
        });
    }
    
    if url_lower.contains("dailymotion.com") {
        return Some(Video {
            url: url.to_string(),
            thumbnail: None,
            platform: Some("Dailymotion".to_string()),
        });
    }
    
    if url_lower.contains("twitch.tv") {
        return Some(Video {
            url: url.to_string(),
            thumbnail: None,
            platform: Some("Twitch".to_string()),
        });
    }
    
    if url_lower.contains(".mp4") || url_lower.contains(".webm") || url_lower.contains(".m3u8") {
        return Some(Video {
            url: url.to_string(),
            thumbnail: None,
            platform: Some("HTML5".to_string()),
        });
    }
    
    None
}

fn extract_youtube_id(url: &str) -> Option<String> {
    if url.contains("youtu.be/") {
        if let Some(pos) = url.rfind('/') {
            let id = &url[pos + 1..];
            if !id.is_empty() && (id.len() == 11 || id.contains('?')) {
                return Some(id.split('?').next().unwrap_or(id).to_string());
            }
        }
    }
    
    if url.contains("youtube.com/embed/") {
        if let Some(pos) = url.rfind("embed/") {
            let rest = &url[pos + 7..];
            let id = rest.split('/').next().unwrap_or(rest).split('?').next().unwrap_or(rest);
            if id.len() == 11 {
                return Some(id.to_string());
            }
        }
    }
    
    if url.contains("youtube.com/watch") {
        if let Some(pos) = url.find("v=") {
            let rest = &url[pos + 2..];
            let id = rest.split('&').next().unwrap_or(rest);
            if id.len() >= 11 {
                return Some(id[..11].to_string());
            }
        }
    }
    
    None
}

fn clean_text(text: &str) -> String {
    let re = Regex::new(r"\n{3,}").unwrap();
    let text = re.replace_all(text, "\n\n");
    
    let re = Regex::new(r" {2,}").unwrap();
    let text = re.replace_all(&text, " ");
    
    text.trim().to_string()
}

fn is_likely_code_or_json(text: &str) -> bool {
    let text_lower = text.to_lowercase();
    
    if text_lower.contains("self.__next_f") || 
       text_lower.contains("__next_f.push") ||
       text_lower.contains("\"@context\":\"https://schema.org") ||
       text_lower.contains("react.fragment") ||
       text_lower.contains("$sreact.") {
        return true;
    }
    
    let comment_patterns = [
        "leave a reply", "cancel reply", "document.addeventlistener",
        "jetpack_remote_comment", "commentforms", "getelementsbyclassname",
        "domcontentloaded", "getelementbyid", "getelementsbytagname",
        ".addEventListener", "var ", "function ", "=> {", ") {",
        "javascript:", "onclick", "onload", "onerror"
    ];
    
    let pattern_count: usize = comment_patterns.iter()
        .filter(|p| text_lower.contains(*p))
        .count();
    
    if pattern_count >= 2 {
        return true;
    }
    
    if text_lower.contains("document.") && text_lower.contains("=") {
        return true;
    }
    
    if text.len() > 100 {
        let mut bracket_count = 0;
        let mut brace_count = 0;
        let mut quote_count = 0;
        let mut colon_count = 0;
        
        for c in text.chars() {
            match c {
                '[' | ']' => bracket_count += 1,
                '{' | '}' => brace_count += 1,
                '"' => quote_count += 1,
                ':' => colon_count += 1,
                _ => {}
            }
        }
        
        let total = text.len() as f64;
        let symbol_ratio = (bracket_count + brace_count + quote_count + colon_count) as f64 / total;
        
        if symbol_ratio > 0.25 && bracket_count > 5 && brace_count > 5 {
            return true;
        }
    }
    
    false
}

fn filter_article_text(text: &str, filters: &[String]) -> String {
    let ui_phrases = [
        "leave a reply", "cancel reply", "save my name", "save my email",
        "notify me of", "comment here", "post comment", "add comment",
        "your email", "your name", "website", "your comment",
        "required fields are marked", "you may use these html tags",
        "subscribe to", "no comments", "be the first to comment",
        "loading", "submitting", "posting", "log in to post",
        "search for", "never miss a hack", "follow on facebook",
        "follow on twitter", "follow on youtube", "follow on rss",
        "contact us", "our columns", "more from this category",
        "hackaday podcast", "this week in security", "ask hackaday",
        "hackaday links", "blood tests could provide", "plenty of patches",
        "replacing old gear", "phrack calls", "llm be good for",
        "comments", "our columns", "featured posts", "popular posts",
        "related posts", "trending", "recent posts", "latest news",
        "breaking news", "read more", "view all", "see more",
        "older posts", "newer posts", "next post", "previous post"
    ];
    
    let paragraphs: Vec<String> = text
        .split("\n\n")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .filter(|p| !is_likely_code_or_json(p))
        .filter(|p| {
            let p_lower = p.to_lowercase();
            
            for phrase in ui_phrases {
                if p_lower.contains(phrase) {
                    return false;
                }
            }
            
            for filter in filters {
                if p_lower.contains(&filter.to_lowercase()) {
                    return false;
                }
            }
            
            if p.len() < 50 {
                return false;
            }
            
            let word_count = p.split_whitespace().count();
            if word_count < 5 {
                return false;
            }
            
            let alpha_count: usize = p.chars().filter(|c| c.is_alphabetic()).count();
            let total_count = p.len();
            if total_count > 0 && (alpha_count as f64 / total_count as f64) < 0.3 {
                return false;
            }
            
            if is_metadata_line(&p_lower) {
                return false;
            }
            
            true
        })
        .collect();
    
    paragraphs.join("\n\n")
}

fn is_metadata_line(text: &str) -> bool {
    if text.contains(" comments") && text.chars().filter(|c| !c.is_numeric()).count() < 20 {
        return true;
    }
    
    let metadata_patterns = [
        "was a", "were a", "is a", "are a",
        "the ", "story", "more than", "missed it"
    ];
    let word_count = text.split_whitespace().count();
    
    if word_count < 30 {
        let pattern_count: usize = metadata_patterns.iter()
            .filter(|p| text.contains(*p))
            .count();
        
        if pattern_count >= 2 && text.len() < 200 {
            return true;
        }
    }
    
    false
}

fn clean_url(url: &str) -> String {
    let url = url.trim();
    if url.starts_with("//") {
        format!("https:{}", url)
    } else if url.starts_with('/') {
        String::new()
    } else if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        String::new()
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![fetch_feed, extract_content])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}