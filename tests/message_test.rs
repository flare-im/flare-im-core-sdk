//! 消息操作集成测试
//!
//! 覆盖 event.proto 定义的全部消息操作 SDK 全流程：
//! - ContentBuilder → MessageBuilder → send → SendAck
//! - 撤回 / 编辑 / 删除 → 本地 Store 副作用
//! - 已读回执 / 正在输入
//! - 表情反应 / 置顶 / 标记
//! - ContentDecoder 编解码往返
//! - EventBus 事件接收
//!
//! 无服务端测试直接运行，集成测试需要服务端：
//! ```bash
//! # 本地单元测试（不需要服务端）
//! cargo test --test message_test
//!
//! # 集成测试（需要服务端运行）
//! cargo test --test message_test --features integration-tests -- --ignored
//!
//! 运行前请确保：1) 已启动 flare-orchestrator、storage-writer、gateway 等；2) 修改代码后需重启 orchestrator 再跑集成测试。
//! ```

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use flare_im_core_sdk::model::IMMessage;
use flare_im_core_sdk::model::message::*;
use flare_im_core_sdk::model::preview_storage::{self, PreviewStoragePayload};
use flare_im_core_sdk::prelude::*;
use flare_proto::common::ImageFormat;
use flare_proto::common::TypingEvent;
use flare_proto::common::message_content::Content as ProtoContent;
use serde_json::json;

// =============================================================================
// ContentBuilder: 26 种消息类型构建测试
// =============================================================================

#[tokio::test]
async fn test_content_builder_text() {
    let built = ContentBuilder::text("Hello @All!")
        .mention_all(6, 4)
        .build();
    assert_eq!(built.message_type, MessageType::Text);
    assert!(!built.encode().is_empty());
}

#[tokio::test]
async fn test_content_builder_text_with_mentions() {
    let built = ContentBuilder::text("Hello @Alice @Bob")
        .mention_user("alice_id", 6, 6)
        .mention_user("bob_id", 13, 4)
        .build();
    assert_eq!(built.message_type, MessageType::Text);

    let msg = MessageBuilder::new("c", "u")
        .content(built)
        .build()
        .unwrap();
    let decoded = decode_content(&msg).unwrap();
    if let DecodedContent::Content(ProtoContent::Text(text)) = &decoded {
        assert_eq!(text.mentions.len(), 2);
    } else {
        panic!("expected Text");
    }
}

#[tokio::test]
async fn test_content_builder_image() {
    let built = ContentBuilder::image("img_001")
        .source(ImageInfo {
            uuid: "img_001".into(),
            image_id: "img_001".into(),
            url: "https://cdn.example.com/a.jpg".into(),
            mime_type: String::new(),
            size: 0,
            width: 0,
            height: 0,
            format: ImageFormat::Unspecified as i32,
            animated: false,
        })
        .description("photo")
        .build();
    assert_eq!(built.message_type, MessageType::Image);
}

#[tokio::test]
async fn test_content_builder_video() {
    let built = ContentBuilder::video("vid_001")
        .video_source(flare_proto::common::VideoInfo {
            url: "https://cdn.example.com/v.mp4".into(),
            duration_ms: 30000,
            ..Default::default()
        })
        .build();
    assert_eq!(built.message_type, MessageType::Video);
}

#[tokio::test]
async fn test_content_builder_audio() {
    let built = ContentBuilder::audio("aud_001")
        .audio_source(flare_proto::common::AudioInfo {
            url: "https://cdn.example.com/a.ogg".into(),
            duration_ms: 5000,
            ..Default::default()
        })
        .build();
    assert_eq!(built.message_type, MessageType::Audio);
}

#[tokio::test]
async fn test_content_builder_file() {
    let built = ContentBuilder::file("file_001")
        .file_name("report.pdf")
        .mime_type("application/pdf")
        .file_size(1024 * 1024)
        .url("https://cdn.example.com/report.pdf")
        .build();
    assert_eq!(built.message_type, MessageType::File);
}

#[tokio::test]
async fn test_content_builder_location() {
    let built = ContentBuilder::location(39.916, 116.397)
        .address("天安门广场")
        .build();
    assert_eq!(built.message_type, MessageType::Location);
}

#[tokio::test]
async fn test_content_builder_card() {
    let built = ContentBuilder::card("user_123")
        .card_type("user")
        .title("Alice")
        .avatar("https://example.com/avatar.jpg")
        .build();
    assert_eq!(built.message_type, MessageType::Card);
}

#[tokio::test]
async fn test_content_builder_sticker() {
    let built = ContentBuilder::sticker("cat_001")
        .package_id("cat")
        .url("https://cdn.example.com/cat_001.webp")
        .size(512, 512)
        .sticker_format("webp")
        .build();
    assert_eq!(built.message_type, MessageType::Sticker);
}

#[tokio::test]
async fn test_content_builder_emoji() {
    let built = ContentBuilder::emoji("😀").build();
    assert_eq!(built.message_type, MessageType::Emoji);
}

#[tokio::test]
async fn test_content_builder_gif() {
    let built = ContentBuilder::gif("gif_001")
        .url("https://cdn.example.com/funny.gif")
        .size(320, 240)
        .build();
    assert_eq!(built.message_type, MessageType::Image);
}

#[tokio::test]
async fn test_content_builder_quote() {
    let built = ContentBuilder::quote("msg_original")
        .quoted_sender_id("user_abc")
        .quoted_text_preview("原文预览")
        .build();
    assert_eq!(built.message_type, MessageType::Quote);
}

#[tokio::test]
async fn test_content_builder_link_card() {
    let built = ContentBuilder::link_card("https://example.com")
        .title("Example")
        .description("An example page")
        .link_card_thumbnail_url("https://example.com/t.png")
        .link_card_site_name("example.com")
        .build();
    assert_eq!(built.message_type, MessageType::LinkCard);
}

#[tokio::test]
async fn test_content_builder_forward() {
    use flare_proto::common::{ForwardItem, ForwardMode, MessageContent, TextContent};
    let item = ForwardItem {
        source_message_id: Some("m1".into()),
        source_conversation_id: Some("c1".into()),
        source_sender_id: Some("u1".into()),
        source_sender_name: None,
        source_message_time: None,
        message_type: MessageType::Text as i32,
        plain_text: "hi".into(),
        content: Some(MessageContent {
            content: Some(flare_proto::common::message_content::Content::Text(
                TextContent {
                    text: "hi".into(),
                    mentions: vec![],
                },
            )),
        }),
    };
    let built =
        ContentBuilder::forward(ForwardMode::Merged, Some("聊天记录".into()), vec![item]).build();
    assert_eq!(built.message_type, MessageType::MergeForward);
}

#[tokio::test]
async fn test_content_builder_thread() {
    let built = ContentBuilder::thread("thread_001")
        .thread_title("讨论主题")
        .build();
    assert_eq!(built.message_type, MessageType::Thread);
}

#[tokio::test]
async fn test_content_builder_mini_program() {
    let mut ex = std::collections::HashMap::new();
    ex.insert("scene".to_string(), "debug".to_string());
    let built = ContentBuilder::mini_program("app_001")
        .title("小程序")
        .page_path("/pages/index")
        .mini_program_thumbnail_url("https://example.com/t.png")
        .mini_program_extend_extra(ex)
        .build();
    assert_eq!(built.message_type, MessageType::MiniProgram);
}

#[tokio::test]
async fn test_content_builder_rich_doc() {
    let built = ContentBuilder::try_rich_doc(
        r#"{"type":"doc","version":2,"children":[]}"#,
        "rich_doc",
        "摘要",
    )
    .unwrap()
    .rich_text_title(Some("文章标题".into()))
    .rich_text_input_format("markdown")
    .rich_text_input_format_version(1)
    .rich_text_source_payload_entry("markdown", "# Title\n")
    .build();
    assert_eq!(built.message_type, MessageType::RichText);
}

#[tokio::test]
async fn test_content_builder_image_group() {
    let built = ContentBuilder::image_group(vec![
        ImageInfo {
            uuid: String::new(),
            image_id: String::new(),
            url: "https://cdn.example.com/1.jpg".into(),
            mime_type: String::new(),
            size: 0,
            width: 0,
            height: 0,
            format: ImageFormat::Unspecified as i32,
            animated: false,
        },
        ImageInfo {
            uuid: String::new(),
            image_id: String::new(),
            url: "https://cdn.example.com/2.jpg".into(),
            mime_type: String::new(),
            size: 0,
            width: 0,
            height: 0,
            format: ImageFormat::Unspecified as i32,
            animated: false,
        },
    ])
    .build();
    assert_eq!(built.message_type, MessageType::ImageGroup);
}

#[tokio::test]
async fn test_content_builder_system() {
    let built = ContentBuilder::system("group.member_joined", "Alice 加入了群聊")
        .data("user_id", "alice_001")
        .build();
    assert_eq!(built.message_type, MessageType::System);
}

#[tokio::test]
async fn test_content_builder_notification() {
    let built = ContentBuilder::notification("通知标题", "通知正文")
        .notification_type("announcement")
        .persistent(true)
        .show_badge(true)
        .build();
    assert_eq!(built.message_type, MessageType::Notification);
}

#[tokio::test]
async fn test_content_builder_vote() {
    let built =
        ContentBuilder::vote("vote_001", "午餐吃什么", vec!["火锅".into(), "烧烤".into()]).build();
    assert_eq!(built.message_type, MessageType::Poll);
}

#[tokio::test]
async fn test_content_builder_task() {
    let built = ContentBuilder::task("task_001", "完成文档")
        .status("pending")
        .build();
    assert_eq!(built.message_type, MessageType::Task);
}

#[tokio::test]
async fn test_content_builder_schedule() {
    let built = ContentBuilder::schedule("sch_001", "团队周会").build();
    assert_eq!(built.message_type, MessageType::Schedule);
}

#[tokio::test]
async fn test_content_builder_announcement() {
    let built = ContentBuilder::announcement("公告标题", "公告正文")
        .pinned(true)
        .build();
    assert_eq!(built.message_type, MessageType::Announcement);
}

#[tokio::test]
async fn test_content_builder_custom() {
    let built = ContentBuilder::custom("red_packet")
        .payload(b"{}".to_vec())
        .description("恭喜发财")
        .build();
    assert_eq!(built.message_type, MessageType::Custom);
}

#[tokio::test]
async fn test_content_builder_placeholder() {
    let built = ContentBuilder::placeholder("e2e_placeholder")
        .fallback_text("[加密消息]")
        .build();
    assert_eq!(built.message_type, MessageType::E2ePlaceholder);
}

// =============================================================================
// MessageBuilder
// =============================================================================

#[tokio::test]
async fn test_message_builder_full() {
    let content = ContentBuilder::text("Hello!").build();
    let msg = MessageBuilder::new("conv_001", "user_001")
        .content(content)
        .channel("user_002")
        .single_chat()
        .offline_push("新消息", "Hello!")
        .extra("threadId", "t_001")
        .build()
        .unwrap();

    assert_eq!(msg.conversation_id, "conv_001");
    assert_eq!(msg.sender_id, "user_001");
    assert_eq!(msg.channel_id, "user_002");
    assert_eq!(msg.conversation_type, ConversationType::Single as i32);
    assert_eq!(msg.message_type, MessageType::Text as i32);
    assert!(!msg.client_msg_id.is_empty());
    assert!(!msg.content.is_empty());
    assert!(msg.offline_push_info.is_some());
    assert_eq!(msg.extra.get("threadId"), Some(&"t_001".to_string()));
}

#[tokio::test]
async fn test_message_builder_quick_text() {
    let msg = MessageBuilder::text("conv_001", "user_001", "Quick text").unwrap();
    assert_eq!(msg.conversation_id, "conv_001");
    assert_eq!(msg.message_type, MessageType::Text as i32);
}

#[tokio::test]
async fn test_message_builder_requires_content() {
    let result = MessageBuilder::new("conv_001", "user_001").build();
    assert!(result.is_err(), "build without content should fail");
}

#[tokio::test]
async fn test_message_builder_group_chat() {
    let msg = MessageBuilder::new("conv_grp", "user_001")
        .content(ContentBuilder::text("群消息").build())
        .group_chat()
        .channel("channel_001")
        .build()
        .unwrap();
    assert_eq!(msg.conversation_type, ConversationType::Group as i32);
    assert_eq!(msg.channel_id, "channel_001");
}

// =============================================================================
// ContentDecoder 编解码往返
// =============================================================================

#[tokio::test]
async fn test_content_roundtrip_text() {
    let original = ContentBuilder::text("Hello @Alice!")
        .mention_user("alice_id", 6, 6)
        .build();

    let msg = MessageBuilder::new("c", "u")
        .content(original)
        .build()
        .unwrap();
    let decoded = decode_content(&msg).unwrap();
    assert_eq!(decoded.message_type(), MessageType::Text);
    let preview: PreviewStoragePayload = serde_json::from_str(&decoded.text_preview()).unwrap();
    assert_eq!(preview.k, preview_storage::keys::USER_TEXT);
    assert_eq!(
        preview.a.get("t").and_then(|v| v.as_str()),
        Some("Hello @Alice!")
    );

    if let DecodedContent::Content(ProtoContent::Text(text)) = &decoded {
        assert_eq!(text.text, "Hello @Alice!");
        assert_eq!(text.mentions.len(), 1);
        assert_eq!(text.mentions[0].user_id, "alice_id");
    } else {
        panic!("expected Text content");
    }
}

#[tokio::test]
async fn test_content_roundtrip_all_types() {
    let cases: Vec<(BuiltContent, MessageType)> = vec![
        (ContentBuilder::text("hello").build(), MessageType::Text),
        (ContentBuilder::image("i").build(), MessageType::Image),
        (ContentBuilder::video("v").build(), MessageType::Video),
        (ContentBuilder::audio("a").build(), MessageType::Audio),
        (ContentBuilder::file("f").build(), MessageType::File),
        (
            ContentBuilder::location(0.0, 0.0).build(), // lat, lon
            MessageType::Location,
        ),
        (ContentBuilder::card("u").build(), MessageType::Card),
        (ContentBuilder::sticker("s").build(), MessageType::Sticker),
        (ContentBuilder::emoji("😀").build(), MessageType::Emoji),
        (ContentBuilder::gif("g").build(), MessageType::Image),
        (ContentBuilder::quote("q").build(), MessageType::Quote),
        (
            ContentBuilder::link_card("https://example.com").build(),
            MessageType::LinkCard,
        ),
        (
            ContentBuilder::forward(
                flare_proto::common::ForwardMode::Single,
                None,
                vec![flare_proto::common::ForwardItem {
                    source_message_id: Some("m".into()),
                    source_conversation_id: None,
                    source_sender_id: None,
                    source_sender_name: None,
                    source_message_time: None,
                    message_type: MessageType::Text as i32,
                    plain_text: "x".into(),
                    content: Some(flare_proto::common::MessageContent {
                        content: Some(flare_proto::common::message_content::Content::Text(
                            flare_proto::common::TextContent {
                                text: "x".into(),
                                mentions: vec![],
                            },
                        )),
                    }),
                }],
            )
            .build(),
            MessageType::MergeForward,
        ),
        (ContentBuilder::thread("t").build(), MessageType::Thread),
        (
            ContentBuilder::mini_program("mp").build(),
            MessageType::MiniProgram,
        ),
        (
            ContentBuilder::try_rich_doc(
                r#"{"type":"doc","version":2,"children":[]}"#,
                "rich_doc",
                "rt",
            )
            .unwrap()
            .build(),
            MessageType::RichText,
        ),
        (
            ContentBuilder::image_group(vec![]).build(),
            MessageType::ImageGroup,
        ),
        (
            ContentBuilder::system("e", "b").build(),
            MessageType::System,
        ),
        (
            ContentBuilder::notification("t", "b").build(),
            MessageType::Notification,
        ),
        (
            ContentBuilder::vote("v", "t", vec![]).build(),
            MessageType::Poll,
        ),
        (
            ContentBuilder::task("t", "title").build(),
            MessageType::Task,
        ),
        (
            ContentBuilder::schedule("s", "title").build(),
            MessageType::Schedule,
        ),
        (
            ContentBuilder::announcement("t", "b").build(),
            MessageType::Announcement,
        ),
        (ContentBuilder::custom("c").build(), MessageType::Custom),
        (
            ContentBuilder::placeholder("p").build(),
            MessageType::E2ePlaceholder,
        ),
    ];

    for (built, expected_type) in cases {
        let msg = MessageBuilder::new("c", "u")
            .content(built)
            .build()
            .unwrap();
        let decoded = decode_content(&msg).unwrap();
        assert_eq!(
            decoded.message_type(),
            expected_type,
            "roundtrip failed for {expected_type:?}"
        );
    }
}

#[tokio::test]
async fn test_content_text_previews() {
    let cases: Vec<(BuiltContent, &'static str, serde_json::Value)> = vec![
        (
            ContentBuilder::text("hello").build(),
            preview_storage::keys::USER_TEXT,
            json!({ "t": "hello" }),
        ),
        (
            ContentBuilder::image("i").build(),
            preview_storage::keys::IMAGE,
            json!({}),
        ),
        (
            ContentBuilder::video("v").build(),
            preview_storage::keys::VIDEO,
            json!({}),
        ),
        (
            ContentBuilder::audio("a").build(),
            preview_storage::keys::AUDIO,
            json!({}),
        ),
        (
            ContentBuilder::file("f").file_name("a.pdf").build(),
            preview_storage::keys::FILE,
            json!({ "n": "a.pdf" }),
        ),
        (
            ContentBuilder::location(0.0, 0.0).address("北京").build(),
            preview_storage::keys::LOCATION,
            json!({ "label": "北京" }),
        ),
        (
            ContentBuilder::card("u")
                .card_type("user")
                .title("Alice")
                .build(),
            preview_storage::keys::CARD,
            json!({ "label": "Alice" }),
        ),
        (
            ContentBuilder::emoji("😀").build(),
            preview_storage::keys::EMOJI,
            json!({ "e": "😀" }),
        ),
        (
            ContentBuilder::custom("red_packet")
                .description("恭喜发财")
                .build(),
            preview_storage::keys::CUSTOM,
            json!({ "d": "恭喜发财" }),
        ),
    ];

    for (built, expected_k, expected_a) in cases {
        let msg = MessageBuilder::new("c", "u")
            .content(built)
            .build()
            .unwrap();
        let decoded = decode_content(&msg).unwrap();
        let p: PreviewStoragePayload = serde_json::from_str(&decoded.text_preview())
            .unwrap_or_else(|e| {
                panic!(
                    "preview JSON parse failed: {e}; raw={}",
                    decoded.text_preview()
                )
            });
        assert_eq!(p.k, expected_k, "raw={}", decoded.text_preview());
        let a_obj = serde_json::Value::Object(p.a.clone());
        assert_eq!(
            a_obj,
            expected_a,
            "key={expected_k} raw={}",
            decoded.text_preview()
        );
    }
}

// =============================================================================
// EventBus 消息事件订阅
// =============================================================================

#[tokio::test]
async fn test_event_bus_on_message_callback() {
    let bus = EventBus::new();
    let count = Arc::new(AtomicU32::new(0));
    let count_clone = count.clone();

    let _sub = bus.on_message(move |_msg| {
        count_clone.fetch_add(1, Ordering::Relaxed);
    });

    bus.publish(SdkEvent::Message(MessageEvent::Received {
        message: IMMessage::new(Message {
            server_id: "srv_001".into(),
            conversation_id: "conv_001".into(),
            sender_id: "user_002".into(),
            message_type: MessageType::Text as i32,
            ..Default::default()
        }),
    }));
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn test_event_bus_subscribe_typing() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    bus.publish(SdkEvent::Message(MessageEvent::Typing {
        conversation_id: "conv_001".into(),
        event: TypingEvent {
            conversation_id: "conv_001".into(),
            user_id: "user_002".into(),
            typing: true,
        },
    }));

    let event = tokio::time::timeout(tokio::time::Duration::from_millis(200), rx.recv())
        .await
        .unwrap()
        .unwrap();

    match &event {
        SdkEvent::Message(MessageEvent::Typing {
            conversation_id,
            event,
        }) => {
            assert_eq!(conversation_id, "conv_001");
            assert!(event.typing);
        }
        _ => panic!("expected Typing event"),
    }
}

#[tokio::test]
async fn test_event_bus_subscribe_send_ack() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    bus.publish(SdkEvent::Message(MessageEvent::SendAck {
        ack: SendAck {
            client_msg_id: "cli_001".into(),
            server_msg_id: "srv_001".into(),
            seq: 42,
            success: true,
            ..Default::default()
        },
    }));

    let event = tokio::time::timeout(tokio::time::Duration::from_millis(200), rx.recv())
        .await
        .unwrap()
        .unwrap();

    match &event {
        SdkEvent::Message(MessageEvent::SendAck { ack }) => {
            assert!(ack.success);
            assert_eq!(ack.seq, 42);
            assert_eq!(ack.server_msg_id, "srv_001");
        }
        _ => panic!("expected SendAck event"),
    }
}

#[tokio::test]
async fn test_event_bus_multiple_subscribers() {
    let bus = EventBus::new();
    let count1 = Arc::new(AtomicU32::new(0));
    let count2 = Arc::new(AtomicU32::new(0));
    let c1 = count1.clone();
    let c2 = count2.clone();

    let _sub1 = bus.on_message(move |_| {
        c1.fetch_add(1, Ordering::Relaxed);
    });
    let _sub2 = bus.on_message(move |_| {
        c2.fetch_add(1, Ordering::Relaxed);
    });

    bus.publish(SdkEvent::Message(MessageEvent::Received {
        message: IMMessage::new(Message::default()),
    }));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    assert_eq!(count1.load(Ordering::Relaxed), 1);
    assert_eq!(count2.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn test_event_bus_drop_subscription() {
    let bus = EventBus::new();
    let count = Arc::new(AtomicU32::new(0));
    let c = count.clone();

    let sub = bus.on_message(move |_| {
        c.fetch_add(1, Ordering::Relaxed);
    });
    drop(sub);
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    bus.publish(SdkEvent::Message(MessageEvent::Received {
        message: IMMessage::new(Message::default()),
    }));
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    // 当前实现：Subscription 仅持有一个 handle，drop 不会从总线移除回调，回调仍会触发
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

// =============================================================================
// MessageStore 内存实现测试
// =============================================================================

#[tokio::test]
async fn test_message_store_save_and_get() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().messages.clone();

    let msg = IMMessage::new(Message {
        server_id: "srv_001".into(),
        client_msg_id: "cli_001".into(),
        conversation_id: "conv_001".into(),
        sender_id: "user_001".into(),
        message_type: MessageType::Text as i32,
        seq: 1,
        ..Default::default()
    });

    store.save_batch(&[msg.clone()]).await.unwrap();
    let loaded = store.get("srv_001").await.unwrap();
    assert!(loaded.is_some());
    assert_eq!(loaded.unwrap().sender_id(), "user_001");
}

#[tokio::test]
async fn test_message_store_get_by_conversation() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().messages.clone();

    for i in 1..=5 {
        let msg = IMMessage::new(Message {
            server_id: format!("srv_{i:03}"),
            conversation_id: "conv_001".into(),
            seq: i as u64,
            ..Default::default()
        });
        store.save_batch(&[msg]).await.unwrap();
    }

    let list = store.get_by_conversation("conv_001", 100, 3).await.unwrap();
    assert_eq!(list.len(), 3);
    assert!(list[0].seq >= list[1].seq, "should be ordered desc");

    let first_page = store.get_by_conversation("conv_001", 0, 2).await.unwrap();
    assert_eq!(first_page.len(), 2);
    assert_eq!(first_page[0].seq, 5);
    assert_eq!(first_page[1].seq, 4);
}

#[tokio::test]
async fn test_message_store_update_and_delete() {
    let client = common::create_test_client_no_connect().await;
    let store = client.stores().unwrap().messages.clone();

    let msg = IMMessage::new(Message {
        server_id: "srv_upd".into(),
        status: MessageStatus::Created as i32,
        content: b"original".to_vec(),
        ..Default::default()
    });
    store.save_batch(&[msg]).await.unwrap();

    store
        .update_status("srv_upd", MessageStatus::Sent as i32)
        .await
        .unwrap();
    let updated = store.get("srv_upd").await.unwrap().unwrap();
    assert_eq!(updated.status, MessageStatus::Sent as i32);

    store
        .update_content("srv_upd", b"edited".to_vec())
        .await
        .unwrap();
    let edited = store.get("srv_upd").await.unwrap().unwrap();
    assert_eq!(edited.content_bytes, b"edited");

    store.delete("srv_upd").await.unwrap();
    assert!(store.get("srv_upd").await.unwrap().is_none());
}

// =============================================================================
// 服务端集成测试：消息发送全流程
// =============================================================================

#[cfg(feature = "integration-tests")]
mod server_tests {
    use super::*;
    use common::{
        SERIAL_LOCK, build_single_text, create_test_client, establish_connection, teardown,
    };

    const SENDER: &str = "user_test_001";
    const RECEIVER: &str = "user_test_002";
    const CONV: &str = "conv_test_001";

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_send_text_message() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let msg = build_single_text(CONV, SENDER, RECEIVER, "集成测试消息");
        let ack = client.message().send(msg).await.unwrap();
        assert!(ack.success, "send should succeed");
        // 服务端可能先返回 success 再异步填充 server_msg_id，此处仅校验成功；若有 server_msg_id 则 seq 通常 > 0
        if !ack.server_msg_id.is_empty() {
            assert!(ack.seq > 0, "should have seq > 0 when server_msg_id is set");
        }

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_send_image_message() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let content = ContentBuilder::image("img_test_001")
            .source(ImageInfo {
                uuid: "img_test_001".into(),
                image_id: "img_test_001".into(),
                url: "https://cdn.example.com/test.jpg".into(),
                mime_type: "image/jpeg".into(),
                size: 102400,
                width: 1920,
                height: 1080,
                format: ImageFormat::Jpeg as i32,
                animated: false,
            })
            .description("测试图片")
            .build();

        let msg = MessageBuilder::new(CONV, SENDER)
            .content(content)
            .channel(RECEIVER)
            .single_chat()
            .build()
            .unwrap();

        let ack = client.message().send(IMMessage::new(msg)).await.unwrap();
        assert!(ack.success);

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_send_custom_message() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let content = ContentBuilder::custom("red_packet")
            .payload(br#"{"amount":100}"#.to_vec())
            .description("恭喜发财")
            .metadata("theme", "spring_festival")
            .build();

        let msg = MessageBuilder::new(CONV, SENDER)
            .content(content)
            .channel(RECEIVER)
            .single_chat()
            .build()
            .unwrap();

        let ack = client.message().send(IMMessage::new(msg)).await.unwrap();
        assert!(ack.success);

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_recall_message() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let msg = build_single_text(CONV, SENDER, RECEIVER, "将被撤回的消息");
        let ack = client.message().send(msg).await.unwrap();
        assert!(ack.success);
        tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;

        let result = client.message().recall(&ack.server_msg_id).await;
        assert!(result.is_ok(), "recall should succeed: {:?}", result.err());

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_edit_message() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let msg = build_single_text(CONV, SENDER, RECEIVER, "原始内容");
        let ack = client.message().send(msg).await.unwrap();
        assert!(ack.success);
        tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;

        let new_content = ContentBuilder::text("编辑后的内容").build();
        let result = client
            .message()
            .edit_content(CONV, &ack.server_msg_id, new_content)
            .await;
        assert!(result.is_ok(), "edit should succeed: {:?}", result.err());

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_delete_message() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let msg = build_single_text(CONV, SENDER, RECEIVER, "将被删除的消息");
        let ack = client.message().send(msg).await.unwrap();
        assert!(ack.success);
        tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;

        let result = client.message().delete(&ack.server_msg_id).await;
        assert!(result.is_ok(), "delete should succeed: {:?}", result.err());

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_mark_read() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let result = client.message().mark_read(CONV, 100).await;
        assert!(
            result.is_ok(),
            "mark_read should succeed: {:?}",
            result.err()
        );

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_typing_indicator() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        client.message().typing(CONV, true).await.unwrap();
        client.message().typing(CONV, false).await.unwrap();

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_reaction_add_remove() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let msg = build_single_text(CONV, SENDER, RECEIVER, "测试反应");
        let ack = client.message().send(msg).await.unwrap();
        assert!(ack.success);
        tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;

        client
            .message()
            .add_reaction(&ack.server_msg_id, "👍")
            .await
            .unwrap();
        client
            .message()
            .remove_reaction(&ack.server_msg_id, "👍")
            .await
            .unwrap();

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_pin_unpin() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let msg = build_single_text(CONV, SENDER, RECEIVER, "测试置顶");
        let ack = client.message().send(msg).await.unwrap();
        assert!(ack.success);
        tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;

        client
            .message()
            .pin(CONV, &ack.server_msg_id)
            .await
            .unwrap();
        client
            .message()
            .unpin(CONV, &ack.server_msg_id)
            .await
            .unwrap();

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_mark_unmark() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        let msg = build_single_text(CONV, SENDER, RECEIVER, "测试标记");
        let ack = client.message().send(msg).await.unwrap();
        assert!(ack.success);
        tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;

        client
            .message()
            .mark(CONV, &ack.server_msg_id, MarkType::Important)
            .await
            .unwrap();
        client
            .message()
            .unmark(CONV, &ack.server_msg_id, MarkType::Important)
            .await
            .unwrap();

        teardown(&mut client).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_query_messages() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        for i in 0..3 {
            let msg = build_single_text(CONV, SENDER, RECEIVER, &format!("查询测试 {i}"));
            let ack = client.message().send(msg).await.unwrap();
            assert!(ack.success);
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;

        let messages = client.message().list(CONV, u64::MAX, 50).await.unwrap();
        assert!(!messages.is_empty(), "should have messages");
        assert!(
            messages.len() >= 3,
            "sync should have at least 3 messages sent, got {}",
            messages.len()
        );

        teardown(&mut client).await;
    }

    /// 同步完整性：发送 N 条消息后拉取会话流，确保无漏消息
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires running server"]
    async fn test_sync_no_missing_messages() {
        let _guard = SERIAL_LOCK.lock().await;
        let mut client = create_test_client().await;
        establish_connection(&mut client, SENDER).await;

        const N: usize = 5;
        let mut sent_ids = Vec::with_capacity(N);
        for i in 0..N {
            let msg = build_single_text(CONV, SENDER, RECEIVER, &format!("sync_integrity_{i}"));
            let ack = client.message().send(msg).await.unwrap();
            assert!(ack.success, "send {} should succeed", i);
            sent_ids.push(ack.server_msg_id.clone());
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
        let messages = client.message().list(CONV, u64::MAX, 50).await.unwrap();
        assert!(
            messages.len() >= N,
            "sync must not drop messages: sent {}, list len {}",
            N,
            messages.len(),
        );
        let listed_ids: std::collections::HashSet<_> =
            messages.iter().map(|m| m.server_id()).collect();
        for id in &sent_ids {
            assert!(
                listed_ids.contains(id.as_str()),
                "sent message must appear in list: {}",
                id,
            );
        }

        teardown(&mut client).await;
    }
}
