//! Pins how a message and its content read from JSON, for every input form the SDK reads them in:
//! JSON text and a `serde_json::Value`, through the core readers (`IMMessage::from_json_str`,
//! `IMMessage::from_json_value`, `Elem::from_json_value`; content on its own as text through
//! `serde_json`, since the SDK only reads it as text inside a message) and through the binding
//! entry points that read a message (`message_from_json_str`, `message_from_params`), a list of
//! messages (`json_vec_message`) or content (`built_content_from_value`).
//!
//! Each case records what reading it gives: the read value as canonical JSON (object keys sorted),
//! or the error's category and text. The results live in `fixtures/message-json-golden.json`; a
//! change in any of them fails here. After a deliberate change, regenerate with
//! `FLARE_UPDATE_GOLDEN=1` and review the diff.

use std::path::Path;

use flare_im_core_sdk::content::DecodedContent;
use flare_im_core_sdk::content::message_elem::{Elem, decoded_content_to_elem};
use flare_im_core_sdk::model::IMMessage;
use flare_im_core_sdk::serde_json::{self, Map, Value, json};
use flare_im_core_sdk_bindings_runtime::dispatch_support::{
    built_content_from_value, json_vec_message, message_from_json_str, message_from_params,
};

/// Content documents: every kind, the tag in other positions, and the ways a document can be
/// wrong (tag, fields, types, ranges, duplicates, shape, syntax).
const CONTENTS: &[(&str, &str)] = &[
    (
        "text",
        r#"{"contentType":"text","text":"hi","mentions":[]}"#,
    ),
    (
        "text with a mention",
        r#"{"contentType":"text","text":"@b hi","mentions":[{"type":1,"userId":"b","userIds":["b"],"roleId":"","start":0,"length":2}]}"#,
    ),
    (
        "text, tag last",
        r#"{"text":"hi","mentions":[],"contentType":"text"}"#,
    ),
    (
        "text, tag in the middle",
        r#"{"text":"hi","contentType":"text","mentions":[]}"#,
    ),
    (
        "text, unicode and escapes",
        r#"{"contentType":"text","text":"你好 \"q\" \\ \n 🙂","mentions":[]}"#,
    ),
    (
        "text, escaped tag key",
        r#"{"contentType":"text","text":"hi","mentions":[]}"#,
    ),
    (
        "text, unknown field",
        r#"{"contentType":"text","text":"hi","mentions":[],"extra":{"a":[1,2]}}"#,
    ),
    (
        "image, full",
        r#"{"contentType":"image","source":{"uuid":"u","imageId":"img-1","url":"https://x/a.png","mimeType":"image/png","size":10,"width":2,"height":3,"format":1,"animated":false,"blurhash":"LEHV6n"},"thumbnail":{"uuid":"t","url":"","mimeType":"image/png","size":1,"width":1,"height":1},"description":"d","durationMs":40}"#,
    ),
    (
        "image, null parts",
        r#"{"contentType":"image","source":null,"thumbnail":null,"description":""}"#,
    ),
    (
        "image, duplicate field in a nested struct",
        r#"{"contentType":"image","source":{"uuid":"u","uuid":"v","url":"","mimeType":"","size":1,"width":1,"height":1},"thumbnail":null,"description":""}"#,
    ),
    (
        "image, size as a float",
        r#"{"contentType":"image","source":{"uuid":"u","url":"","mimeType":"","size":1.5,"width":1,"height":1},"thumbnail":null,"description":""}"#,
    ),
    (
        "image, width out of range",
        r#"{"contentType":"image","source":{"uuid":"u","url":"","mimeType":"","size":1,"width":3000000000,"height":1},"thumbnail":null,"description":""}"#,
    ),
    (
        "video",
        r#"{"contentType":"video","videoId":"v1","source":{"uuid":"u","url":"","mimeType":"video/mp4","size":9,"durationMs":1000,"width":4,"height":3},"cover":null,"description":""}"#,
    ),
    (
        "audio",
        r#"{"contentType":"audio","audioId":"a1","source":{"uuid":"u","url":"","mimeType":"audio/aac","size":9,"durationMs":1000},"description":""}"#,
    ),
    (
        "file",
        r#"{"contentType":"file","fileId":"f1","fileName":"a.pdf","mimeType":"application/pdf","fileSize":12,"url":"","description":""}"#,
    ),
    (
        "location, optional parts",
        r#"{"contentType":"location","latitude":1.5,"longitude":-2,"title":"t","address":"a","zoom":12,"snapshotUrl":"https://x/s.png"}"#,
    ),
    (
        "location, zoom out of range",
        r#"{"contentType":"location","latitude":1.5,"longitude":2,"title":"t","address":"a","zoom":300}"#,
    ),
    (
        "card, map with a duplicate key",
        r#"{"contentType":"card","cardType":"user","id":"u2","title":"B","subtitle":"","avatar":"","extra":{"k":"1","k":"2"}}"#,
    ),
    (
        "sticker, defaults",
        r#"{"contentType":"sticker","stickerId":"s","url":"https://x/s.webp","width":1,"height":1}"#,
    ),
    (
        "emoji",
        r#"{"contentType":"emoji","emoji":"🙂","description":"smile","extra":{}}"#,
    ),
    (
        "quote, nested content",
        r#"{"contentType":"quote","quotedMessageId":"m1","quotedSenderId":"b","quotedTextPreview":"hi","quotedContent":{"contentType":"text","text":"hi","mentions":[]},"currentContent":null}"#,
    ),
    (
        "quote, nested content without a tag",
        r#"{"contentType":"quote","quotedMessageId":"m1","quotedSenderId":"b","quotedTextPreview":"hi","quotedContent":{"text":"hi","mentions":[]},"currentContent":null}"#,
    ),
    (
        "link card",
        r#"{"contentType":"link_card","url":"https://x","title":"t","description":"d","thumbnailUrl":"","siteName":"x"}"#,
    ),
    (
        "forward, nested items",
        r#"{"contentType":"forward","mode":1,"title":"chat","items":[{"sourceMessageId":"m1","sourceSenderName":"B","sourceMessageTimeMs":5,"messageType":1,"plainText":"hi","content":{"contentType":"text","text":"hi","mentions":[]}},{"messageType":2,"plainText":"[image]"}]}"#,
    ),
    (
        "thread, two levels of nesting",
        r#"{"contentType":"thread","threadId":"t1","threadTitle":"T","rootContent":{"contentType":"quote","quotedMessageId":"m","quotedSenderId":"b","quotedTextPreview":"","quotedContent":{"contentType":"text","text":"x","mentions":[]},"currentContent":null},"metadata":{"a":"b"}}"#,
    ),
    (
        "mini program",
        r#"{"contentType":"mini_program","appId":"app","title":"t","pagePath":"/p","thumbnailUrl":"","extra":{}}"#,
    ),
    (
        "rich text, optional parts",
        r#"{"contentType":"rich_text","docJson":"{}","contentSchema":"flare.richdoc.v2","plainText":"p","inputFormat":"markdown","inputFormatVersion":1,"sourcePayload":{"m":"*p*"},"title":"T","searchText":"p","renderHintsJson":"{}"}"#,
    ),
    (
        "image group",
        r#"{"contentType":"image_group","images":[{"uuid":"u","url":"","mimeType":"","size":1,"width":1,"height":1}],"description":"","metadata":{}}"#,
    ),
    (
        "system, payload bytes",
        r#"{"contentType":"system","eventKind":"member_joined","body":"b","data":{"u":"1"},"payload":[1,2,255]}"#,
    ),
    (
        "system, payload as a string",
        r#"{"contentType":"system","eventKind":"k","body":"b","data":{},"payload":"AQI="}"#,
    ),
    (
        "system, payload byte out of range",
        r#"{"contentType":"system","eventKind":"k","body":"b","data":{},"payload":[256]}"#,
    ),
    (
        "notification",
        r#"{"contentType":"notification","title":"t","body":"b","notificationType":"n","data":{},"targetUserIds":["u"],"targetRoleId":"","notifyAll":false,"persistent":true,"showInList":true,"showBadge":false,"playSound":false}"#,
    ),
    (
        "vote",
        r#"{"contentType":"vote","voteId":"v","title":"t","options":["a","b"],"metadata":{},"participantUserIds":["u"]}"#,
    ),
    (
        "task",
        r#"{"contentType":"task","taskId":"t","title":"t","status":"open","metadata":{}}"#,
    ),
    (
        "schedule",
        r#"{"contentType":"schedule","scheduleId":"s","title":"t","startTime":1,"endTime":2,"metadata":{}}"#,
    ),
    (
        "schedule, negative time",
        r#"{"contentType":"schedule","scheduleId":"s","title":"t","startTime":-1,"endTime":2,"metadata":{}}"#,
    ),
    (
        "announcement",
        r#"{"contentType":"announcement","title":"t","body":"b","pinned":true,"metadata":{}}"#,
    ),
    (
        "custom, payload",
        r#"{"contentType":"custom","type":"x.y","payload":[7],"description":"d","metadata":{"k":"v"}}"#,
    ),
    (
        "placeholder",
        r#"{"contentType":"placeholder","reason":"unsupported","fallbackText":"?","metadata":{}}"#,
    ),
    ("tag missing", r#"{"text":"hi","mentions":[]}"#),
    (
        "tag unknown",
        r#"{"contentType":"nope","text":"hi","mentions":[]}"#,
    ),
    (
        "tag in another case",
        r#"{"contentType":"Text","text":"hi","mentions":[]}"#,
    ),
    (
        "tag as a number",
        r#"{"contentType":0,"text":"hi","mentions":[]}"#,
    ),
    (
        "tag as null",
        r#"{"contentType":null,"text":"hi","mentions":[]}"#,
    ),
    (
        "tag as an object",
        r#"{"contentType":{"text":null},"text":"hi","mentions":[]}"#,
    ),
    (
        "tag twice",
        r#"{"contentType":"text","contentType":"image","text":"hi","mentions":[]}"#,
    ),
    (
        "field twice",
        r#"{"contentType":"text","text":"a","text":"b","mentions":[]}"#,
    ),
    (
        "field twice before the tag",
        r#"{"text":"a","text":"b","contentType":"text","mentions":[]}"#,
    ),
    (
        "field of the wrong type",
        r#"{"contentType":"text","text":5,"mentions":[]}"#,
    ),
    (
        "required field missing",
        r#"{"contentType":"text","text":"hi"}"#,
    ),
    (
        "null for a required list",
        r#"{"contentType":"text","text":"hi","mentions":null}"#,
    ),
    ("array form", r#"["text","hi",[]]"#),
    ("array form with an extra element", r#"["text","hi",[],1]"#),
    ("array form, empty", r#"[]"#),
    ("empty object", r#"{}"#),
    ("a string", r#""text""#),
    ("null", r#"null"#),
    ("truncated", r#"{"contentType":"text","text":"hi""#),
    (
        "trailing characters",
        r#"{"contentType":"text","text":"hi","mentions":[]} x"#,
    ),
    (
        "trailing comma",
        r#"{"contentType":"text","text":"hi","mentions":[],}"#,
    ),
    (
        "unknown tag, then a syntax error",
        r#"{"contentType":"nope","text":"#,
    ),
    (
        "type error, then a syntax error",
        r#"{"contentType":"text","text":5,"#,
    ),
];

/// A message document with every required field; `{content}` is replaced by a content member.
const MESSAGE: &str = r#"{"serverId":"s1","clientMsgId":"c1","conversationId":"conv-1","conversationType":1,"channelId":"","senderId":"u1","source":0,"conversationSeq":7,"createdAt":1700000000000,"clientCreatedAt":1700000000001,"messageType":1,"senderName":"A","senderAvatar":"","senderDisplayName":"A","status":2,"isRead":false,"isRecalled":false,"isEdited":false,"mentionUsers":[],"mentionAll":false,"attributes":{"k":"v"},"version":1,"updatedAt":1700000000002{content}}"#;

fn message_with(content: &str) -> String {
    MESSAGE.replace("{content}", content)
}

/// Message documents: no content, content of every shape above that changes the outcome, and the
/// ways the message itself can be wrong; plus the `{"message": …}` wrapper the bindings accept.
fn messages() -> Vec<(String, String)> {
    let mut out = vec![
        ("no content".to_string(), message_with("")),
        (
            "content null".to_string(),
            message_with(r#","content":null"#),
        ),
        (
            "message, unknown field".to_string(),
            message_with(r#","unknownField":[1,{"a":2}]"#),
        ),
        (
            "message, field twice".to_string(),
            message_with(r#","clientMsgId":"c2""#),
        ),
        (
            "message, map with a duplicate key".to_string(),
            MESSAGE
                .replace(
                    r#""attributes":{"k":"v"}"#,
                    r#""attributes":{"k":"v","k":"w"}"#,
                )
                .replace("{content}", ""),
        ),
        (
            "message, field of the wrong type".to_string(),
            MESSAGE
                .replace(r#""conversationSeq":7"#, r#""conversationSeq":"7""#)
                .replace("{content}", ""),
        ),
        (
            "message, required field missing".to_string(),
            MESSAGE
                .replace(r#""serverId":"s1","#, "")
                .replace("{content}", ""),
        ),
        (
            "message, local state and reactions".to_string(),
            message_with(
                r#","localState":{"sending":true,"failed":false,"isLocal":true,"uploading":false,"uploadProgress":0,"sortTs":9},"reactions":[{"emoji":"👍","count":1,"user_ids":["u2"]}],"extensions":{"x":[1,2]}"#,
            ),
        ),
        (
            "message, truncated".to_string(),
            message_with("").trim_end_matches('}').to_string(),
        ),
        ("message, an array".to_string(), "[]".to_string()),
        ("message, not JSON".to_string(), "message".to_string()),
        (
            "wrapped message".to_string(),
            format!(
                r#"{{"message":{}}}"#,
                message_with(r#","content":{"contentType":"text","text":"hi","mentions":[]}"#)
            ),
        ),
        (
            "wrapped message, invalid".to_string(),
            format!(
                r#"{{"message":{}}}"#,
                message_with(r#","content":{"contentType":"nope"}"#)
            ),
        ),
        (
            "wrapper without a message".to_string(),
            r#"{"other":1}"#.to_string(),
        ),
    ];
    for (name, content) in CONTENTS {
        out.push((
            format!("content: {name}"),
            message_with(&format!(r#","content":{content}"#)),
        ));
    }
    out
}

fn canonical<T: flare_im_core_sdk::serde::Serialize>(value: &T) -> String {
    // serde_json's Map is a BTreeMap here, so going through Value sorts object keys and makes
    // the output independent of HashMap order.
    let value = serde_json::to_value(value).expect("serialize read value");
    serde_json::to_string(&value).expect("render read value")
}

fn json_result<T: flare_im_core_sdk::serde::Serialize>(result: serde_json::Result<T>) -> String {
    match result {
        Ok(value) => format!("ok {}", canonical(&value)),
        Err(error) => format!("err {:?}: {error}", error.classify()),
    }
}

fn sdk_result<T>(result: flare_im_core_sdk::Result<T>, ok: impl FnOnce(T) -> String) -> String {
    match result {
        Ok(value) => format!("ok {}", ok(value)),
        Err(error) => format!("err {error}"),
    }
}

/// Keeps the fixture readable: a result equal to the one in `other` is written as `label`.
fn same_as(result: String, other: &Value, label: &str) -> String {
    if other.as_str() == Some(result.as_str()) {
        label.to_string()
    } else {
        result
    }
}

fn read_content_text(text: &str) -> String {
    json_result(serde_json::from_str::<Elem>(text))
}

fn read_content_value(value: Value) -> String {
    json_result(Elem::from_json_value(value))
}

fn read_message_text(text: &str) -> String {
    json_result(IMMessage::from_json_str(text))
}

fn read_message_value(value: Value) -> String {
    json_result(IMMessage::from_json_value(value))
}

fn results() -> Value {
    let mut contents = Vec::new();
    for (name, text) in CONTENTS {
        let mut row = Map::new();
        row.insert("name".into(), json!(name));
        row.insert("input".into(), json!(text));
        row.insert("text".into(), json!(read_content_text(text)));
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            row.insert(
                "value".into(),
                json!(same_as(
                    read_content_value(value.clone()),
                    &row["text"],
                    "= text"
                )),
            );
            row.insert(
                "built content".into(),
                json!(sdk_result(built_content_from_value(&value), |built| {
                    let back = built
                        .inner
                        .content
                        .clone()
                        .map(|content| DecodedContent::Content(Box::new(content)))
                        .and_then(|decoded| decoded_content_to_elem(&decoded));
                    format!("{:?} {}", built.message_type, canonical(&back))
                })),
            );
        }
        contents.push(Value::Object(row));
    }
    let mut messages_out = Vec::new();
    for (name, text) in messages() {
        let mut row = Map::new();
        row.insert("name".into(), json!(name));
        row.insert("input".into(), json!(text));
        let read_text = read_message_text(&text);
        row.insert("text".into(), json!(read_text));
        row.insert(
            "binding text".into(),
            json!(same_as(
                sdk_result(message_from_json_str(&text), |m| canonical(&m)),
                &json!(read_text),
                "= text"
            )),
        );
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            let read_value = read_message_value(value.clone());
            row.insert(
                "value".into(),
                json!(same_as(read_value.clone(), &json!(read_text), "= text")),
            );
            row.insert(
                "binding params".into(),
                json!(same_as(
                    sdk_result(message_from_params(&value), |m| canonical(&m)),
                    &json!(read_value),
                    "= value"
                )),
            );
            // The same document as the only element of a message list.
            let list = json!({ "sourceMessages": [value] });
            row.insert(
                "binding list".into(),
                json!(same_as(
                    sdk_result(json_vec_message(&list, "sourceMessages"), |messages| {
                        match messages.as_slice() {
                            [message] => canonical(message),
                            other => format!("{} messages", other.len()),
                        }
                    }),
                    &json!(read_value),
                    "= value"
                )),
            );
        }
        messages_out.push(Value::Object(row));
    }
    json!({ "contents": contents, "messages": messages_out })
}

#[test]
fn messages_and_their_content_read_from_json_as_pinned() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/message-json-golden.json");
    let actual = format!(
        "{}\n",
        serde_json::to_string_pretty(&results()).expect("render results")
    );
    if std::env::var_os("FLARE_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().expect("fixture directory"))
            .expect("create fixtures");
        std::fs::write(&path, &actual).expect("write golden file");
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("read golden file");
    if expected != actual {
        let expected: Value = serde_json::from_str(&expected).expect("parse golden file");
        let actual: Value = serde_json::from_str(&actual).expect("parse results");
        let mut changes = Vec::new();
        for section in ["contents", "messages"] {
            let (Some(old), Some(new)) = (expected[section].as_array(), actual[section].as_array())
            else {
                continue;
            };
            if old.len() != new.len() {
                changes.push(format!("{section}: {} cases, now {}", old.len(), new.len()));
            }
            for (old_row, new_row) in old.iter().zip(new) {
                if old_row != new_row {
                    changes.push(format!(
                        "{section} / {}:\n  was {}\n  now {}",
                        new_row["name"], old_row, new_row
                    ));
                }
            }
        }
        panic!(
            "reading messages from JSON changed in {} place(s):\n{}",
            changes.len(),
            changes.join("\n")
        );
    }
}
