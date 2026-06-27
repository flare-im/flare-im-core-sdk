use std::collections::HashMap;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use flare_im_core_sdk::event::{MessageEvent, MessageEventType, SdkEventType};
use flare_im_core_sdk::prelude::Codec;
use flare_im_core_sdk::prelude::in_memory_im_provider;
use flare_im_core_sdk::prelude::*;
use flare_proto::common::data_packet::Payload as DataPacketPayload;
use flare_proto::common::message_content::Content as MessageContentPayload;
use flare_proto::common::{
    CustomData, DataPacket, Message, MessageContent, MessageType, TextContent,
};
use prost::Message as ProstMessage;

fn sample_im_message(seq: u64) -> IMMessage {
    IMMessage::new(sample_proto_message(seq))
}

fn sample_proto_message(seq: u64) -> Message {
    Message {
        server_id: format!("srv-{seq}"),
        conversation_id: "c1".to_string(),
        client_msg_id: format!("cli-{seq}"),
        sender_id: "u1".to_string(),
        source: flare_proto::common::MessageSource::User as i32,
        created_at: seq as i64,
        conversation_seq: seq,
        conversation_type: flare_proto::common::ConversationType::Single as i32,
        message_type: MessageType::Text as i32,
        channel_id: "u2".to_string(),
        content: Some(MessageContent {
            content: Some(MessageContentPayload::Text(TextContent {
                text: "hello from perf baseline".to_string(),
                mentions: Vec::new(),
            })),
        }),
        status: 1,
        ..Default::default()
    }
}

fn bench_event_bus_publish(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_bus_publish_steady_state");
    for subscriber_count in [0usize, 1, 10, 100] {
        let bus = EventBus::new();
        let mut subscribers = (0..subscriber_count)
            .map(|_| bus.subscribe())
            .collect::<Vec<_>>();

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(subscriber_count),
            &subscriber_count,
            |b, _| {
                b.iter(|| {
                    bus.publish(black_box(SdkEvent::Message(MessageEvent::Received {
                        message: Box::new(sample_im_message(1)),
                    })));
                    for subscriber in subscribers.iter_mut() {
                        black_box(subscriber.try_recv().expect("receiver active"));
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_event_filter_try_recv(c: &mut Criterion) {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    c.benchmark_group("event_filter")
        .throughput(Throughput::Elements(1))
        .bench_function("try_recv_matching", |b| {
            b.iter(|| {
                bus.publish(SdkEvent::Message(MessageEvent::Received {
                    message: Box::new(sample_im_message(1)),
                }));
                black_box(rx.try_recv().expect("receiver active"));
            });
        });
}

fn bench_message_send_prepare(c: &mut Criterion) {
    c.benchmark_group("message_send")
        .throughput(Throughput::Elements(1))
        .bench_function("prepare_text_message", |b| {
            b.iter(|| {
                let mut message = sample_im_message(black_box(1));
                message.materialize_encoded_content_from_elem();
                let proto = message.to_proto();
                black_box(proto.encode_to_vec());
            });
        });
}

fn bench_message_store_save(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let stores = in_memory_im_provider();
    let messages = (0..100)
        .map(|i| sample_im_message(i + 1))
        .collect::<Vec<_>>();

    c.benchmark_group("message_send")
        .throughput(Throughput::Elements(messages.len() as u64))
        .bench_function("memory_store_save_batch_100", |b| {
            b.iter(|| {
                rt.block_on(async {
                    stores
                        .messages
                        .save_batch(black_box(messages.as_slice()))
                        .await
                        .expect("save batch");
                });
            });
        });
}

fn bench_message_send_local_pipeline(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let stores = in_memory_im_provider();
    let messages = (0..100)
        .map(|i| sample_im_message(i + 1))
        .collect::<Vec<_>>();

    c.benchmark_group("message_send")
        .throughput(Throughput::Elements(messages.len() as u64))
        .bench_function("local_prepare_store_encode_100", |b| {
            b.iter(|| {
                rt.block_on(async {
                    let mut prepared = Vec::with_capacity(messages.len());
                    for message in black_box(messages.as_slice()) {
                        let mut message = message.clone();
                        message.materialize_encoded_content_from_elem();
                        black_box(message.to_proto().encode_to_vec());
                        prepared.push(message);
                    }
                    stores
                        .messages
                        .save_batch(prepared.as_slice())
                        .await
                        .expect("save batch");
                });
            });
        });
}

fn bench_message_receive_batch(c: &mut Criterion) {
    let bus = EventBus::new();
    let mut rx = bus.subscribe_event_type(SdkEventType::Message(MessageEventType::Received));
    let messages = (0..100)
        .map(|i| sample_im_message(i + 1))
        .collect::<Vec<_>>();

    c.benchmark_group("message_receive")
        .throughput(Throughput::Elements(messages.len() as u64))
        .bench_function("event_bus_publish_and_drain_100", |b| {
            b.iter(|| {
                for message in messages.iter() {
                    bus.publish(SdkEvent::Message(MessageEvent::Received {
                        message: Box::new(message.clone()),
                    }));
                }
                for _ in 0..messages.len() {
                    black_box(rx.try_recv().expect("receiver active"));
                }
            });
        });
}

fn bench_sync_messages_thousand(c: &mut Criterion) {
    let bus = EventBus::new();
    let mut rx = bus.subscribe_event_type(SdkEventType::Message(MessageEventType::Received));
    let messages = (0..1000)
        .map(|i| sample_im_message(i + 1))
        .collect::<Vec<_>>();

    c.benchmark_group("sync_messages")
        .throughput(Throughput::Elements(messages.len() as u64))
        .bench_function("event_bus_publish_and_drain_1000", |b| {
            b.iter(|| {
                for message in messages.iter() {
                    bus.publish(SdkEvent::Message(MessageEvent::Received {
                        message: Box::new(message.clone()),
                    }));
                }
                for _ in 0..messages.len() {
                    black_box(rx.try_recv().expect("receiver active"));
                }
            });
        });
}

fn bench_event_json_serialization(c: &mut Criterion) {
    let message = sample_im_message(1);
    let batch = (0..1000)
        .map(|i| sample_im_message(i + 1))
        .collect::<Vec<_>>();

    let mut group = c.benchmark_group("event_json_serialization");
    group.throughput(Throughput::Elements(1));
    group.bench_function("message_received_payload", |b| {
        b.iter(|| {
            let payload = serde_json::json!({
                "type": "message.received",
                "payload": black_box(&message),
            });
            black_box(serde_json::to_string(&payload).expect("event payload serializes"));
        });
    });

    group.throughput(Throughput::Elements(batch.len() as u64));
    group.bench_function("sync_messages_1000_payload", |b| {
        b.iter(|| {
            let payload = serde_json::json!({
                "type": "message.received_batch",
                "messages": black_box(&batch),
            });
            black_box(serde_json::to_string(&payload).expect("event batch serializes"));
        });
    });
    group.finish();
}

fn bench_protocol_codec(c: &mut Criterion) {
    let data_packet = DataPacket {
        payload: Some(DataPacketPayload::UserCustom(CustomData {
            r#type: "im.message.send".to_string(),
            payload: sample_proto_message(1).encode_to_vec(),
            attributes: HashMap::new(),
        })),
    };
    let encoded = data_packet.encode_to_vec();
    let codec = ProtobufCodec;

    let mut group = c.benchmark_group("protocol_codec");
    group.throughput(Throughput::Bytes(encoded.len() as u64));
    group.bench_function("decode_data_packet", |b| {
        b.iter(|| {
            black_box(
                codec
                    .decode_server(black_box(encoded.as_slice()))
                    .expect("data packet decodes"),
            );
        });
    });
    group.finish();
}

criterion_group!(
    perf_baseline,
    bench_event_bus_publish,
    bench_event_filter_try_recv,
    bench_message_send_prepare,
    bench_message_store_save,
    bench_message_send_local_pipeline,
    bench_message_receive_batch,
    bench_sync_messages_thousand,
    bench_event_json_serialization,
    bench_protocol_codec
);
criterion_main!(perf_baseline);
