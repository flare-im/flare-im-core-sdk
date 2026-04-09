//! CommonMark（pulldown-cmark）→ RichDoc v2 JSON `Value`。
//!
//! 映射细则见仓库 `docs/RICHDOC_V2.md` 第 9 节。

use pulldown_cmark::{CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};
use serde_json::{json, Value};

use super::RichDocV2Error;

pub fn markdown_to_doc_value(md: &str) -> Result<Value, RichDocV2Error> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(md, opts);
    let mut p = MdParser {
        events: parser.peekable(),
    };
    let children = p.parse_document()?;
    Ok(json!({
        "type": "doc",
        "version": 2u32,
        "children": children,
    }))
}

struct MdParser<I: Iterator> {
    events: std::iter::Peekable<I>,
}

impl<'a, I> MdParser<I>
where
    I: Iterator<Item = Event<'a>>,
{
    fn parse_document(&mut self) -> Result<Vec<Value>, RichDocV2Error> {
        let mut blocks = Vec::new();
        while self.events.peek().is_some() {
            if let Some(b) = self.next_block()? {
                blocks.push(b);
            }
        }
        Ok(blocks)
    }

    fn next_block(&mut self) -> Result<Option<Value>, RichDocV2Error> {
        match self.events.next() {
            None => Ok(None),
            Some(Event::Start(Tag::Paragraph)) => {
                let inl = self.parse_inlines_until(TagEnd::Paragraph)?;
                Ok(Some(json!({
                    "type": "paragraph",
                    "children": inl,
                })))
            }
            Some(Event::Start(Tag::Heading { level, .. })) => {
                let end = TagEnd::Heading(level);
                let lvl = heading_level_num(level);
                let inl = self.parse_inlines_until(end)?;
                Ok(Some(json!({
                    "type": "heading",
                    "level": lvl,
                    "children": inl,
                })))
            }
            Some(Event::Start(Tag::BlockQuote(k))) => {
                let end = TagEnd::BlockQuote(k);
                let inner = self.parse_blocks_until(end)?;
                Ok(Some(json!({
                    "type": "quote",
                    "children": inner,
                })))
            }
            Some(Event::Start(Tag::CodeBlock(kind))) => {
                let (lang, body) = self.parse_code_block_body(kind)?;
                let text_node = json!({
                    "type": "text",
                    "text": body,
                });
                let mut code = json!({
                    "type": "code_block",
                    "children": [text_node],
                });
                if let Some(obj) = code.as_object_mut() {
                    if let Some(l) = lang {
                        obj.insert("language".into(), json!(l));
                    }
                }
                Ok(Some(code))
            }
            Some(Event::Start(Tag::List(start))) => {
                let ordered = start.is_some();
                let items = self.parse_list_items(ordered)?;
                let ty = if ordered {
                    "ordered_list"
                } else {
                    "bullet_list"
                };
                Ok(Some(json!({
                    "type": ty,
                    "children": items,
                })))
            }
            Some(Event::Start(Tag::Table(_))) => {
                self.skip_current_table()?;
                Ok(Some(json!({
                    "type": "custom_block",
                    "custom_type": "markdown_table",
                    "children": [],
                })))
            }
            Some(Event::Rule) => Ok(Some(json!({"type": "divider"}))),
            Some(Event::End(_)) => Ok(None),
            Some(Event::Html(_)) | Some(Event::InlineHtml(_)) => self.next_block(),
            Some(Event::FootnoteReference(_)) => self.next_block(),
            Some(Event::SoftBreak) | Some(Event::HardBreak) => self.next_block(),
            Some(Event::Text(_)) | Some(Event::Code(_)) => self.next_block(),
            Some(Event::InlineMath(s)) | Some(Event::DisplayMath(s)) => {
                // 数学扩展：降级为纯文本节点（后续可改为 custom_inline）
                Ok(Some(json!({
                    "type": "paragraph",
                    "children": [{"type": "text", "text": s.to_string()}],
                })))
            }
            Some(Event::TaskListMarker(_)) => self.next_block(),
            Some(Event::Start(_)) => {
                self.skip_balanced_unknown()?;
                self.next_block()
            }
        }
    }

    fn parse_list_items(&mut self, _ordered: bool) -> Result<Vec<Value>, RichDocV2Error> {
        let mut items = Vec::new();
        loop {
            match self.events.peek() {
                Some(Event::End(TagEnd::List(_))) => {
                    self.events.next();
                    break;
                }
                Some(Event::Start(Tag::Item)) => {
                    self.events.next();
                    let mut item_children = Vec::new();
                    loop {
                        match self.events.peek() {
                            Some(Event::End(TagEnd::Item)) => {
                                self.events.next();
                                break;
                            }
                            None => {
                                return Err(RichDocV2Error::Markdown(
                                    "unclosed list item".into(),
                                ));
                            }
                            _ => {
                                if let Some(b) = self.next_block()? {
                                    item_children.push(b);
                                }
                            }
                        }
                    }
                    items.push(json!({
                        "type": "list_item",
                        "children": item_children,
                    }));
                }
                _ => break,
            }
        }
        Ok(items)
    }

    fn parse_blocks_until(&mut self, end: TagEnd) -> Result<Vec<Value>, RichDocV2Error> {
        let mut out = Vec::new();
        loop {
            match self.events.peek() {
                None => {
                    return Err(RichDocV2Error::Markdown(
                        "unclosed block container".into(),
                    ));
                }
                Some(Event::End(e)) if *e == end => {
                    self.events.next();
                    break;
                }
                _ => {
                    if let Some(b) = self.next_block()? {
                        out.push(b);
                    }
                }
            }
        }
        Ok(out)
    }

    fn parse_code_block_body(
        &mut self,
        kind: CodeBlockKind<'_>,
    ) -> Result<(Option<String>, String), RichDocV2Error> {
        let lang = match kind {
            CodeBlockKind::Fenced(l) => Some(l.to_string()),
            CodeBlockKind::Indented => None,
        };
        let mut body = String::new();
        loop {
            match self.events.next() {
                Some(Event::Text(t)) => body.push_str(&t),
                Some(Event::End(TagEnd::CodeBlock)) => break,
                None => {
                    return Err(RichDocV2Error::Markdown(
                        "unclosed code block".into(),
                    ));
                }
                _ => {}
            }
        }
        Ok((lang, body))
    }

    fn parse_inlines_until(&mut self, end: TagEnd) -> Result<Vec<Value>, RichDocV2Error> {
        let mut b = InlineBuf::default();
        loop {
            match self.events.next() {
                None => {
                    return Err(RichDocV2Error::Markdown(
                        "unclosed inline container".into(),
                    ));
                }
                Some(Event::End(e)) if e == end => break,
                Some(Event::Text(t)) => b.push_text(&t),
                Some(Event::Code(t)) => {
                    b.flush_text();
                    b.out.push(json!({
                        "type": "inline_code",
                        "text": t.to_string(),
                    }));
                }
                Some(Event::SoftBreak) => {
                    b.flush_text();
                    b.out.push(json!({"type": "hard_break"}));
                }
                Some(Event::HardBreak) => {
                    b.flush_text();
                    b.out.push(json!({"type": "hard_break"}));
                }
                Some(Event::Start(Tag::Strong)) => b.push_mark("bold"),
                Some(Event::End(TagEnd::Strong)) => b.pop_mark(),
                Some(Event::Start(Tag::Emphasis)) => b.push_mark("italic"),
                Some(Event::End(TagEnd::Emphasis)) => b.pop_mark(),
                Some(Event::Start(Tag::Strikethrough)) => b.push_mark("strike"),
                Some(Event::End(TagEnd::Strikethrough)) => b.pop_mark(),
                Some(Event::Start(Tag::Link {
                    dest_url,
                    title,
                    link_type: LinkType::Inline,
                    ..
                })) => {
                    b.flush_text();
                    let href = dest_url.to_string();
                    let title = title.to_string();
                    let inner = self.parse_inlines_until(TagEnd::Link)?;
                    b.out.push(json!({
                        "type": "link",
                        "href": href,
                        "title": title,
                        "children": inner,
                    }));
                }
                Some(Event::Start(Tag::Link { .. })) => {
                    b.flush_text();
                    self.skip_until_matching_end(TagEnd::Link)?;
                }
                Some(Event::InlineMath(s)) | Some(Event::DisplayMath(s)) => {
                    b.push_text(&s);
                }
                Some(Event::Html(_)) | Some(Event::InlineHtml(_)) => {}
                _ => {}
            }
        }
        b.flush_text();
        Ok(b.out)
    }

    /// 已消费起始 `Start` 后，跳过直到与 `end` 匹配的 `End`（嵌套平衡）。
    fn skip_until_matching_end(&mut self, end: TagEnd) -> Result<(), RichDocV2Error> {
        let mut depth = 1u32;
        while let Some(ev) = self.events.next() {
            match ev {
                Event::Start(_) => depth += 1,
                Event::End(e) => {
                    depth -= 1;
                    if depth == 0 && e == end {
                        break;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn skip_current_table(&mut self) -> Result<(), RichDocV2Error> {
        while let Some(ev) = self.events.next() {
            if matches!(ev, Event::End(TagEnd::Table)) {
                break;
            }
        }
        Ok(())
    }

    fn skip_balanced_unknown(&mut self) -> Result<(), RichDocV2Error> {
        let mut depth = 1u32;
        while let Some(ev) = self.events.next() {
            match ev {
                Event::Start(_) => depth += 1,
                Event::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn heading_level_num(l: pulldown_cmark::HeadingLevel) -> u8 {
    match l {
        pulldown_cmark::HeadingLevel::H1 => 1,
        pulldown_cmark::HeadingLevel::H2 => 2,
        pulldown_cmark::HeadingLevel::H3 => 3,
        pulldown_cmark::HeadingLevel::H4 => 4,
        pulldown_cmark::HeadingLevel::H5 => 5,
        pulldown_cmark::HeadingLevel::H6 => 6,
    }
}

#[derive(Default)]
struct InlineBuf {
    text: String,
    marks: Vec<&'static str>,
    out: Vec<Value>,
}

impl InlineBuf {
    fn push_mark(&mut self, m: &'static str) {
        self.flush_text();
        self.marks.push(m);
    }

    fn pop_mark(&mut self) {
        self.flush_text();
        let _ = self.marks.pop();
    }

    fn push_text(&mut self, t: &str) {
        self.text.push_str(t);
    }

    fn flush_text(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let marks: Vec<Value> = self
            .marks
            .iter()
            .map(|m| json!({ "type": m }))
            .collect();
        let node = if marks.is_empty() {
            json!({
                "type": "text",
                "text": self.text,
            })
        } else {
            json!({
                "type": "text",
                "text": self.text,
                "marks": marks,
            })
        };
        self.out.push(node);
        self.text.clear();
    }
}
