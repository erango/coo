//! A tiny DOM over quick-xml, enough for WebDAV multistatus responses.
//! Elements are matched by local name (namespaces ignored): DAV, CalDAV and
//! Apple property names don't collide in what we read.

use pigeon_core::{Error, Result};
use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct El {
    pub name: String,
    /// The element's `name` attribute (e.g. `<c:comp name="VEVENT"/>`).
    pub name_attr: Option<String>,
    pub text: String,
    pub children: Vec<El>,
}

impl El {
    pub fn child(&self, name: &str) -> Option<&El> {
        self.children.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.children.iter().filter(move |c| c.name.eq_ignore_ascii_case(name))
    }

    /// First descendant with this name, depth-first.
    pub fn find(&self, name: &str) -> Option<&El> {
        for c in &self.children {
            if c.name.eq_ignore_ascii_case(name) {
                return Some(c);
            }
            if let Some(d) = c.find(name) {
                return Some(d);
            }
        }
        None
    }

    pub fn has(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    pub fn text_of(&self, name: &str) -> Option<String> {
        self.find(name).map(|e| e.text.trim().to_string()).filter(|t| !t.is_empty())
    }
}

fn element(s: &quick_xml::events::BytesStart) -> El {
    let name_attr = s.attributes().flatten().find(|a| local(a.key.as_ref()) == "name").map(|a| a.value.to_string());
    El { name: local(s.name().as_ref()), name_attr, ..Default::default() }
}

fn local(raw: &str) -> String {
    raw.rsplit(':').next().unwrap_or(raw).to_string()
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => name
            .strip_prefix("#x")
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
            .and_then(char::from_u32),
    }
}

pub fn parse(xml: &str) -> Result<El> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<El> = vec![El { name: "#root".into(), ..Default::default() }];
    loop {
        match reader.read_event().map_err(|e| Error::Other(format!("bad XML from server: {e}")))? {
            Event::Start(s) => stack.push(element(&s)),
            Event::Empty(s) => {
                let el = element(&s);
                stack.last_mut().expect("root").children.push(el);
            }
            Event::End(_) => {
                if stack.len() > 1 {
                    let el = stack.pop().expect("non-root");
                    stack.last_mut().expect("root").children.push(el);
                }
            }
            Event::Text(t) => stack.last_mut().expect("root").text.push_str(&t.into_inner()),
            Event::CData(t) => stack.last_mut().expect("root").text.push_str(&t.into_inner()),
            Event::GeneralRef(r) => {
                let name = r.into_inner().into_owned();
                if let Some(c) = entity(&name) {
                    stack.last_mut().expect("root").text.push(c);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    while stack.len() > 1 {
        let el = stack.pop().expect("non-root");
        stack.last_mut().expect("root").children.push(el);
    }
    Ok(stack.pop().expect("root"))
}

/// One `<response>` of a multistatus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resp {
    pub href: String,
    /// HTTP status of the response (or its first propstat), e.g. 200 / 404.
    pub status: u16,
    /// Merged `<prop>` of the 200 propstats.
    pub prop: El,
}

fn status_code(s: &str) -> u16 {
    s.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0)
}

/// Responses plus the top-level `<sync-token>` (sync-collection).
pub fn multistatus(xml: &str) -> Result<(Vec<Resp>, Option<String>)> {
    let root = parse(xml)?;
    let ms = root.find("multistatus").ok_or_else(|| Error::Other("server reply isn't a multistatus".into()))?;
    let mut out = Vec::new();
    for r in ms.all("response") {
        let href = r.child("href").map(|h| h.text.trim().to_string()).unwrap_or_default();
        let mut prop = El { name: "prop".into(), ..Default::default() };
        // Any 200 propstat makes the resource 200; otherwise the
        // response-level status (or the first propstat's).
        let mut status = r.child("status").map(|s| status_code(&s.text)).unwrap_or(0);
        for ps in r.all("propstat") {
            let code = ps.child("status").map(|s| status_code(&s.text)).unwrap_or(200);
            if code == 200 {
                status = 200;
                if let Some(p) = ps.child("prop") {
                    prop.children.extend(p.children.iter().cloned());
                }
            } else if status == 0 {
                status = code;
            }
        }
        out.push(Resp { href, status, prop });
    }
    let token = ms.child("sync-token").map(|t| t.text.trim().to_string()).filter(|t| !t.is_empty());
    Ok((out, token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multistatus_with_namespaces_cdata_and_entities() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:A="http://apple.com/ns/ical/">
  <d:response><d:href>/123/calendars/home/</d:href>
    <d:propstat><d:prop><d:displayname>Home &amp; Family</d:displayname>
      <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
      <A:calendar-color>#FF2968FF</A:calendar-color>
      <cal:supported-calendar-component-set><cal:comp name="VEVENT"/><cal:comp name="VTODO"/></cal:supported-calendar-component-set></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status></d:propstat>
    <d:propstat><d:prop><d:getetag/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
  </d:response>
  <d:response><d:href>/123/calendars/home/gone.ics</d:href><d:status>HTTP/1.1 404 Not Found</d:status></d:response>
  <d:response><d:href>/123/calendars/home/e1.ics</d:href>
    <d:propstat><d:prop><d:getetag>"abc"</d:getetag><cal:calendar-data><![CDATA[BEGIN:VCALENDAR
END:VCALENDAR]]></cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
  <d:sync-token>https://example/sync/42</d:sync-token>
</d:multistatus>"#;
        let (resps, token) = multistatus(xml).unwrap();
        assert_eq!(token.as_deref(), Some("https://example/sync/42"));
        assert_eq!(resps.len(), 3);
        assert_eq!(resps[0].status, 200);
        assert_eq!(resps[0].prop.text_of("displayname").as_deref(), Some("Home & Family"));
        assert!(resps[0].prop.find("resourcetype").unwrap().has("calendar"));
        assert_eq!(resps[0].prop.text_of("calendar-color").as_deref(), Some("#FF2968FF"));
        let comps: Vec<_> = resps[0]
            .prop
            .find("supported-calendar-component-set")
            .unwrap()
            .all("comp")
            .filter_map(|c| c.name_attr.clone())
            .collect();
        assert_eq!(comps, ["VEVENT", "VTODO"]);
        assert_eq!((resps[1].href.as_str(), resps[1].status), ("/123/calendars/home/gone.ics", 404));
        assert!(resps[2].prop.text_of("calendar-data").unwrap().starts_with("BEGIN:VCALENDAR"));
        assert_eq!(resps[2].prop.text_of("getetag").as_deref(), Some("\"abc\""));
    }
}
