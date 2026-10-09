use crate::{Document, DocumentId};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Link {
    pub target: String,
    pub label: String,
}

/// Extracts wiki references using Markdown syntax, excluding code and escaped examples.
pub fn links(content: &str) -> Vec<Link> {
    use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};
    let mut found = vec![];
    let mut current: Option<Link> = None;
    for event in Parser::new_ext(content, Options::ENABLE_WIKILINKS) {
        match event {
            Event::Start(Tag::Link {
                link_type: LinkType::WikiLink { .. },
                dest_url,
                ..
            }) => {
                current = Some(Link {
                    target: dest_url.trim().into(),
                    label: String::new(),
                });
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(link) = &mut current {
                    link.label.push_str(&text);
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some(mut link) = current.take() {
                    if link.label.is_empty() {
                        link.label = link.target.clone();
                    }
                    if !link.target.is_empty() {
                        found.push(link);
                    }
                }
            }
            _ => {}
        }
    }
    found
}

pub fn targets<'a>(target: &str, documents: &'a [Document]) -> Vec<&'a Document> {
    if let Some(id) = target.strip_prefix("id:") {
        let Ok(id) = id.parse::<DocumentId>() else {
            return vec![];
        };
        documents.iter().filter(|d| d.id == id).collect()
    } else {
        documents
            .iter()
            .filter(|d| d.title.to_lowercase() == target.to_lowercase())
            .collect()
    }
}
