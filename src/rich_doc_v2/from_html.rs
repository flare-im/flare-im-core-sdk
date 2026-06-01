//! 受限 HTML 片段 → RichDoc v2（仅 native；wasm 返回 [`RichDocV2Error::HtmlUnavailable`]）。
//!
//! 安全策略：仅解析标签白名单；映射规则见 `docs/RICHDOC_V2.md` 第 10 节。

use serde_json::{Value, json};

use super::RichDocV2Error;

#[cfg(not(target_arch = "wasm32"))]
use ego_tree::NodeRef;
#[cfg(not(target_arch = "wasm32"))]
use scraper::{ElementRef, Html, Node, Selector};

/// `html` 为**片段**（可含多个根节点）；外层不包 `<html>` 也可。
#[cfg(not(target_arch = "wasm32"))]
pub fn html_fragment_to_doc_value(html: &str) -> Result<Value, RichDocV2Error> {
    let parsed = Html::parse_fragment(html);
    let mut children = Vec::new();
    for child in parsed.root_element().children() {
        if let Some(er) = ElementRef::wrap(child) {
            map_element(&er, &mut children)?;
        }
    }
    if children.is_empty() {
        let t = parsed.root_element().text().collect::<String>();
        let t = t.trim();
        if !t.is_empty() {
            children.push(json!({
                "type": "paragraph",
                "children": [{"type": "text", "text": t}],
            }));
        }
    }
    Ok(json!({
        "type": "doc",
        "version": 2u32,
        "children": children,
    }))
}

#[cfg(target_arch = "wasm32")]
pub fn html_fragment_to_doc_value(_html: &str) -> Result<Value, RichDocV2Error> {
    Err(RichDocV2Error::HtmlUnavailable)
}

#[cfg(not(target_arch = "wasm32"))]
fn map_element(el: &ElementRef, out: &mut Vec<Value>) -> Result<(), RichDocV2Error> {
    let name = el.value().name();
    match name {
        "p" => {
            let inl = map_phrasing_children(el)?;
            out.push(json!({"type": "paragraph", "children": inl}));
        }
        "br" => {
            out.push(json!({
                "type": "paragraph",
                "children": [{"type": "hard_break"}],
            }));
        }
        "div" => {
            for c in el.children() {
                if let Some(er) = ElementRef::wrap(c) {
                    map_element(&er, out)?;
                }
            }
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let lvl: u8 = name[1..].parse().unwrap_or(1);
            let inl = map_phrasing_children(el)?;
            out.push(json!({
                "type": "heading",
                "level": lvl,
                "children": inl,
            }));
        }
        "blockquote" => {
            let mut inner = Vec::new();
            for c in el.children() {
                if let Some(er) = ElementRef::wrap(c) {
                    map_element(&er, &mut inner)?;
                }
            }
            out.push(json!({"type": "quote", "children": inner}));
        }
        "pre" => {
            let code = el.text().collect::<String>();
            out.push(json!({
                "type": "code_block",
                "children": [{"type": "text", "text": code}],
            }));
        }
        "ul" => {
            let li_sel = Selector::parse("li")
                .map_err(|e| RichDocV2Error::Html(format!("selector: {e}")))?;
            let mut items = Vec::new();
            for li in el.select(&li_sel) {
                let mut item_children = Vec::new();
                map_element(&li, &mut item_children)?;
                items.push(json!({"type": "list_item", "children": item_children}));
            }
            out.push(json!({"type": "bullet_list", "children": items}));
        }
        "ol" => {
            let li_sel = Selector::parse("li")
                .map_err(|e| RichDocV2Error::Html(format!("selector: {e}")))?;
            let mut items = Vec::new();
            for li in el.select(&li_sel) {
                let mut item_children = Vec::new();
                map_element(&li, &mut item_children)?;
                items.push(json!({"type": "list_item", "children": item_children}));
            }
            out.push(json!({"type": "ordered_list", "children": items}));
        }
        "li" => {
            let inl = map_phrasing_children(el)?;
            if !inl.is_empty() {
                out.push(json!({"type": "paragraph", "children": inl}));
            }
        }
        "hr" => out.push(json!({"type": "divider"})),
        _ => {
            let t = el.text().collect::<String>();
            let t = t.trim();
            if !t.is_empty() {
                out.push(json!({
                    "type": "paragraph",
                    "children": [{"type": "text", "text": t}],
                }));
            }
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn map_phrasing_children(el: &ElementRef) -> Result<Vec<Value>, RichDocV2Error> {
    let mut out = Vec::new();
    for c in el.children() {
        map_phrasing_node(c, &mut out)?;
    }
    Ok(out)
}

#[cfg(not(target_arch = "wasm32"))]
fn map_phrasing_node(node: NodeRef<'_, Node>, out: &mut Vec<Value>) -> Result<(), RichDocV2Error> {
    match node.value() {
        Node::Text(t) => {
            let s: &str = t;
            if !s.is_empty() {
                out.push(json!({"type": "text", "text": s}));
            }
        }
        Node::Element(_) => {
            let el = ElementRef::wrap(node)
                .ok_or_else(|| RichDocV2Error::Html("wrap phrasing".into()))?;
            let name = el.value().name();
            match name {
                "br" => out.push(json!({"type": "hard_break"})),
                "strong" | "b" => {
                    let mut inner = Vec::new();
                    for c in el.children() {
                        map_phrasing_node(c, &mut inner)?;
                    }
                    wrap_marks(&mut inner, "bold");
                    out.extend(inner);
                }
                "em" | "i" => {
                    let mut inner = Vec::new();
                    for c in el.children() {
                        map_phrasing_node(c, &mut inner)?;
                    }
                    wrap_marks(&mut inner, "italic");
                    out.extend(inner);
                }
                "u" => {
                    let mut inner = Vec::new();
                    for c in el.children() {
                        map_phrasing_node(c, &mut inner)?;
                    }
                    wrap_marks(&mut inner, "underline");
                    out.extend(inner);
                }
                "s" | "strike" | "del" => {
                    let mut inner = Vec::new();
                    for c in el.children() {
                        map_phrasing_node(c, &mut inner)?;
                    }
                    wrap_marks(&mut inner, "strike");
                    out.extend(inner);
                }
                "code" => {
                    let t = el.text().collect::<String>();
                    out.push(json!({"type": "inline_code", "text": t}));
                }
                "a" => {
                    let href = el.attr("href").unwrap_or("").to_string();
                    let mut inner = Vec::new();
                    for c in el.children() {
                        map_phrasing_node(c, &mut inner)?;
                    }
                    out.push(json!({
                        "type": "link",
                        "href": href,
                        "title": "",
                        "children": inner,
                    }));
                }
                _ => {
                    for c in el.children() {
                        map_phrasing_node(c, out)?;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn wrap_marks(nodes: &mut [Value], mark: &'static str) {
    for n in nodes.iter_mut() {
        if let Some(obj) = n.as_object_mut()
            && obj.get("type").and_then(|t| t.as_str()) == Some("text")
        {
            obj.insert("marks".into(), json!([{"type": mark}]));
        }
    }
}
