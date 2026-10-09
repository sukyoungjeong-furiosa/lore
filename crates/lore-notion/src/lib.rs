use lore_core::{
    BackendDocument, DocumentContent, DocumentListing, DocumentReference, Error, ErrorKind,
    KnowledgeBackend, Result,
};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::{collections::BTreeSet, io::Read, time::Duration};

pub const API_VERSION: &str = "2026-03-11";

pub fn page_id(input: &str) -> Result<String> {
    let value = if input.starts_with("https://") {
        let url =
            reqwest::Url::parse(input).map_err(|_| Error::invalid("Invalid Notion page URL"))?;
        if !matches!(
            url.host_str(),
            Some("notion.so" | "www.notion.so" | "app.notion.com" | "www.notion.com")
        ) {
            return Err(Error::invalid("Expected a Notion page URL or UUID"));
        }
        let segment = url
            .path_segments()
            .and_then(|mut parts| parts.next_back())
            .ok_or_else(|| Error::invalid("Missing Notion page ID"))?;
        if let Ok(id) = uuid::Uuid::parse_str(segment) {
            return Ok(id.to_string());
        }
        segment
            .get(segment.len().saturating_sub(32)..)
            .unwrap_or("")
            .to_string()
    } else {
        input.to_string()
    };
    uuid::Uuid::parse_str(&value)
        .map(|id| id.to_string())
        .map_err(|_| Error::invalid("Expected a Notion page UUID or page URL"))
}

pub struct NotionBackend {
    identity: String,
    token: String,
    api_url: String,
    client: Client,
}
impl NotionBackend {
    pub fn new(identity: String, token: String, api_url: &str) -> Result<Self> {
        let url = reqwest::Url::parse(api_url).map_err(|_| Error::invalid("Invalid API URL"))?;
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::invalid(
                "API URL requires HTTPS, or HTTP on loopback, without credentials or query",
            ));
        }
        if token.trim().is_empty() {
            return Err(Error::new(
                ErrorKind::PermissionDenied,
                "Notion token is empty",
            ));
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| {
                Error::new(
                    ErrorKind::Unavailable,
                    "Cannot initialize Notion HTTP client",
                )
            })?;
        Ok(Self {
            identity,
            token,
            api_url: api_url.trim_end_matches('/').to_owned(),
            client,
        })
    }
    fn reference(&self, id: &str) -> Result<DocumentReference> {
        Ok(DocumentReference {
            backend: self.identity.clone(),
            id: page_id(id)?,
        })
    }
    fn checked_id(&self, reference: &DocumentReference) -> Result<String> {
        if reference.backend != self.identity {
            return Err(Error::invalid("Document belongs to another backend"));
        }
        page_id(&reference.id)
    }
    fn request(&self, path: &str, body: Option<&Value>) -> Result<Value> {
        for attempt in 0..3 {
            let url = format!("{}{path}", self.api_url);
            let request = match body {
                Some(body) => self.client.post(&url).json(body),
                None => self.client.get(&url),
            };
            let mut response = request
                .bearer_auth(&self.token)
                .header("Notion-Version", API_VERSION)
                .send()
                .map_err(|_| {
                    Error::new(ErrorKind::Unavailable, "Notion request failed or timed out")
                })?;
            let status = response.status().as_u16();
            if status == 429 && attempt < 2 {
                let delay = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1);
                if delay <= 5 {
                    std::thread::sleep(Duration::from_secs(delay));
                    continue;
                }
            }
            if !(200..300).contains(&status) {
                let (kind, message) = match status {
                    401 | 403 => (
                        ErrorKind::PermissionDenied,
                        "Notion rejected access; check the token, capabilities, and page connection",
                    ),
                    404 => (
                        ErrorKind::Unavailable,
                        "Notion resource is missing or inaccessible; this does not establish deletion",
                    ),
                    429 => (
                        ErrorKind::Unavailable,
                        "Notion rate limit reached; retry later",
                    ),
                    _ => (
                        ErrorKind::Unavailable,
                        "Notion request returned an unsuccessful status",
                    ),
                };
                return Err(Error::new(kind, format!("{message} (HTTP {status})")));
            }
            let mut bytes = vec![];
            response
                .by_ref()
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::new(ErrorKind::Unavailable, "Cannot read Notion response"))?;
            if bytes.len() > 8 * 1024 * 1024 {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "Notion response exceeds 8 MB",
                ));
            }
            return serde_json::from_slice(&bytes)
                .map_err(|_| Error::new(ErrorKind::Corrupt, "Notion returned invalid JSON"));
        }
        unreachable!()
    }
    fn listing(&self, path: &str, post: bool) -> Result<Vec<Value>> {
        let mut cursor: Option<String> = None;
        let mut visited = BTreeSet::new();
        let mut result = vec![];
        loop {
            let value = if post {
                let mut body = json!({"page_size":100});
                if let Some(cursor) = &cursor {
                    body["start_cursor"] = json!(cursor);
                }
                self.request(path, Some(&body))?
            } else {
                let mut url = reqwest::Url::parse(&format!("{}{path}", self.api_url)).unwrap();
                url.query_pairs_mut().append_pair("page_size", "100");
                if let Some(cursor) = &cursor {
                    url.query_pairs_mut().append_pair("start_cursor", cursor);
                }
                let full_path = format!("{path}?{}", url.query().unwrap());
                self.request(&full_path, None)?
            };
            let rows = value["results"]
                .as_array()
                .ok_or_else(|| Error::new(ErrorKind::Corrupt, "Notion listing lacks results"))?;
            result.extend(rows.iter().cloned());
            let more = value["has_more"].as_bool().ok_or_else(|| {
                Error::new(ErrorKind::Corrupt, "Notion listing lacks pagination state")
            })?;
            if !more {
                break;
            }
            let next = text(&value, "next_cursor")?.to_owned();
            if !visited.insert(next.clone()) || visited.len() > 1000 {
                return Err(Error::new(
                    ErrorKind::Corrupt,
                    "Notion pagination did not converge",
                ));
            }
            cursor = Some(next);
        }
        Ok(result)
    }
    fn summary(&self, page: &Value) -> Result<BackendDocument> {
        let id = text(page, "id")?;
        let properties = page["properties"]
            .as_object()
            .ok_or_else(|| Error::new(ErrorKind::Corrupt, "Notion page lacks properties"))?;
        let title = properties
            .values()
            .find(|p| p["type"] == "title")
            .and_then(|p| p["title"].as_array())
            .map(|parts| {
                parts
                    .iter()
                    .map(|part| {
                        part["plain_text"]
                            .as_str()
                            .or_else(|| part["text"]["content"].as_str())
                            .unwrap_or("")
                    })
                    .collect::<String>()
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Untitled".into());
        Ok(BackendDocument {
            reference: self.reference(id)?,
            title,
            url: page["url"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| notion_url(id)),
            observed_revision: page["last_edited_time"].as_str().map(str::to_owned),
        })
    }
    fn database_pages(&self, id: &str) -> Result<Vec<BackendDocument>> {
        let database = self.request(&format!("/databases/{}", page_id(id)?), None)?;
        let sources = database["data_sources"]
            .as_array()
            .ok_or_else(|| Error::new(ErrorKind::Corrupt, "Notion database lacks data sources"))?;
        let mut pages = vec![];
        for source in sources {
            let id = page_id(text(source, "id")?)?;
            for page in self.listing(&format!("/data_sources/{id}/query"), true)? {
                pages.push(self.summary(&page)?);
            }
        }
        Ok(pages)
    }
}
impl KnowledgeBackend for NotionBackend {
    fn metadata(&self, reference: &DocumentReference) -> Result<BackendDocument> {
        let id = self.checked_id(reference)?;
        let document = self.summary(&self.request(&format!("/pages/{id}"), None)?)?;
        if document.reference.id != id {
            return Err(Error::new(
                ErrorKind::Corrupt,
                "Notion response identifies a different document",
            ));
        }
        Ok(document)
    }
    fn children(&self, reference: &DocumentReference) -> Result<DocumentListing> {
        let id = self.checked_id(reference)?;
        let mut pending = vec![id.clone()];
        let mut visited = BTreeSet::new();
        let mut documents = std::collections::BTreeMap::new();
        let mut warnings = vec![];
        while let Some(block) = pending.pop() {
            if !visited.insert(block.clone()) {
                continue;
            }
            if visited.len() > 1000 {
                warnings.push("Nested block traversal limit reached".into());
                break;
            }
            let rows = match self.listing(&format!("/blocks/{block}/children"), false) {
                Ok(rows) => rows,
                Err(error) if block != id => {
                    warnings.push(format!("Block {block}: {error}"));
                    continue;
                }
                Err(error) => return Err(error),
            };
            for row in rows {
                let block_id = page_id(text(&row, "id")?)?;
                match text(&row, "type")? {
                    "child_page" => {
                        let title = text(&row["child_page"], "title")?.to_owned();
                        documents.insert(
                            block_id.clone(),
                            BackendDocument {
                                reference: self.reference(&block_id)?,
                                title,
                                url: notion_url(&block_id),
                                observed_revision: None,
                            },
                        );
                    }
                    "child_database" => match self.database_pages(&block_id) {
                        Ok(pages) => {
                            for page in pages {
                                documents.insert(page.reference.id.clone(), page);
                            }
                        }
                        Err(error) => warnings.push(format!("Database {block_id}: {error}")),
                    },
                    _ if row["has_children"] == true => pending.push(block_id),
                    "unsupported" => {
                        warnings.push(format!("Unsupported block {block_id} may hide documents"))
                    }
                    _ => {}
                }
            }
        }
        Ok(DocumentListing {
            documents: documents.into_values().collect(),
            warnings,
        })
    }
    fn read(&self, reference: &DocumentReference) -> Result<DocumentContent> {
        let id = self.checked_id(reference)?;
        let mut document = self.metadata(reference)?;
        let value = self.request(&format!("/pages/{id}/markdown"), None)?;
        let content = text(&value, "markdown")?.to_owned();
        let truncated = value["truncated"].as_bool().ok_or_else(|| {
            Error::new(ErrorKind::Corrupt, "Notion Markdown lacks truncation state")
        })?;
        let unknown = value["unknown_block_ids"].as_array().ok_or_else(|| {
            Error::new(ErrorKind::Corrupt, "Notion Markdown lacks block coverage")
        })?;
        let mut warnings: Vec<String> = value["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|warning| warning["message"].as_str().map(str::to_owned))
            .collect();
        if truncated {
            warnings.push("Content was truncated; open the original in Notion".into());
        }
        if !unknown.is_empty() {
            warnings.push(format!(
                "{} blocks could not be rendered; open the original in Notion",
                unknown.len()
            ));
        }
        let after = self.metadata(reference)?;
        let changed = document.observed_revision != after.observed_revision;
        if changed {
            document.observed_revision = None;
            warnings.push(
                "Page changed during retrieval; content and metadata may describe different states"
                    .into(),
            );
        }
        Ok(DocumentContent {
            document,
            content,
            content_format: "notion-markdown".into(),
            partial: truncated || !unknown.is_empty(),
            warnings,
        })
    }
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| Error::new(ErrorKind::Corrupt, format!("Notion response lacks {key}")))
}
fn notion_url(id: &str) -> String {
    format!("https://www.notion.so/{}", id.replace('-', ""))
}
