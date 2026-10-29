// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Streaming reader for SAML metadata aggregates.
//!
//! Aggregates run to ~110 MiB. Building one DOM for the whole file costs gigabytes;
//! instead we stream with `quick-xml` and materialize a small tree for one
//! `EntityDescriptor` at a time, hand it to a callback, and drop it.

use std::io::BufRead;

use anyhow::{Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};
use quick_xml::NsReader;

use crate::ns;

#[derive(Debug, Clone, Default)]
pub struct Attr {
    pub ns: Option<String>,
    pub name: String,
    pub value: String,
}

/// A namespace-resolved element. `text` holds direct character content only.
#[derive(Debug, Clone, Default)]
pub struct Element {
    pub ns: Option<String>,
    pub name: String,
    pub attrs: Vec<Attr>,
    pub text: String,
    pub children: Vec<Element>,
}

impl Element {
    pub fn is(&self, ns: &str, name: &str) -> bool {
        self.name == name && self.ns.as_deref() == Some(ns)
    }

    /// An unqualified attribute, e.g. `entityID`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.ns.is_none() && a.name == name)
            .map(|a| a.value.as_str())
    }

    /// A namespace-qualified attribute, e.g. `remd:contactType`.
    pub fn attr_ns(&self, ns: &str, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.ns.as_deref() == Some(ns) && a.name == name)
            .map(|a| a.value.as_str())
    }

    pub fn lang(&self) -> Option<&str> {
        self.attr_ns(ns::XML, "lang")
    }

    pub fn children<'a>(&'a self, ns: &'a str, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |c| c.is(ns, name))
    }

    pub fn child(&self, ns: &str, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.is(ns, name))
    }

    pub fn text(&self) -> &str {
        self.text.trim()
    }
}

/// Facts about the aggregate itself, read from the root `EntitiesDescriptor`.
#[derive(Debug, Clone, Default)]
pub struct AggregateInfo {
    pub name: Option<String>,
    pub valid_until: Option<String>,
    pub entity_count: usize,
}

/// Stream `reader`, calling `on_entity` once per `md:EntityDescriptor`.
pub fn for_each_entity<R: BufRead>(
    reader: R,
    mut on_entity: impl FnMut(Element),
) -> Result<AggregateInfo> {
    let mut reader = NsReader::from_reader(reader);
    let mut info = AggregateInfo::default();
    let mut seen_root = false;
    // Open elements inside the current EntityDescriptor; empty when outside one.
    let mut stack: Vec<Element> = Vec::new();
    let mut buf = Vec::new();

    loop {
        let pos = reader.buffer_position();
        let (resolved, event) = reader
            .read_resolved_event_into(&mut buf)
            .with_context(|| format!("XML error after byte {pos}"))?;
        let ns = owned_ns(resolved);
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let is_empty = matches!(event, Event::Empty(_));
                let local = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if !seen_root {
                    seen_root = true;
                    if ns.as_deref() == Some(ns::MD) && local == "EntitiesDescriptor" {
                        let el = element(&reader, ns.clone(), local.clone(), e);
                        info.name = el.attr("Name").map(str::to_owned);
                        info.valid_until = el.attr("validUntil").map(str::to_owned);
                    }
                }
                let starts_entity = ns.as_deref() == Some(ns::MD) && local == "EntityDescriptor";
                if stack.is_empty() && !starts_entity {
                    continue;
                }
                let el = element(&reader, ns, local, e);
                if is_empty {
                    if stack.is_empty() {
                        info.entity_count += 1;
                        on_entity(el);
                    } else {
                        stack.last_mut().unwrap().children.push(el);
                    }
                } else {
                    stack.push(el);
                }
            }
            Event::End(_) => {
                if let Some(done) = stack.pop() {
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(done),
                        None => {
                            info.entity_count += 1;
                            on_entity(done);
                        }
                    }
                }
            }
            Event::Text(ref t) => {
                if let Some(top) = stack.last_mut() {
                    match t.unescape() {
                        Ok(s) => top.text.push_str(&s),
                        Err(_) => top.text.push_str(&String::from_utf8_lossy(t.as_ref())),
                    }
                }
            }
            Event::CData(ref t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&String::from_utf8_lossy(t.as_ref()));
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(info)
}

fn owned_ns(r: ResolveResult) -> Option<String> {
    match r {
        ResolveResult::Bound(Namespace(b)) => Some(String::from_utf8_lossy(b).into_owned()),
        _ => None,
    }
}

fn element<R>(reader: &NsReader<R>, ns: Option<String>, name: String, e: &BytesStart) -> Element {
    let mut attrs = Vec::new();
    for a in e.attributes().with_checks(false).flatten() {
        let key = a.key;
        let raw = key.as_ref();
        if raw == b"xmlns" || raw.starts_with(b"xmlns:") {
            continue;
        }
        let (resolved, local) = reader.resolve_attribute(key);
        let local = String::from_utf8_lossy(local.as_ref()).into_owned();
        // The `xml` prefix is bound by definition and never declared; handle it
        // explicitly rather than relying on resolver behavior.
        let ans = if raw.starts_with(b"xml:") {
            Some(ns::XML.to_owned())
        } else {
            owned_ns(resolved)
        };
        let value = match a.unescape_value() {
            Ok(v) => v.into_owned(),
            Err(_) => String::from_utf8_lossy(&a.value).into_owned(),
        };
        attrs.push(Attr {
            ns: ans,
            name: local,
            value,
        });
    }
    Element {
        ns,
        name,
        attrs,
        text: String::new(),
        children: Vec::new(),
    }
}
