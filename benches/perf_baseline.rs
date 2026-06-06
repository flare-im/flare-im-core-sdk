use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use flare_im_core_sdk::adapter_prelude::{Codec, ProtobufCodec};
use flare_im_core_sdk::core::SdkState;
use flare_im_core_sdk::core::event::{ConnectionEvent, EventBus, SdkEvent};
use flare_im_core_sdk::model::{ContentBuilder, decode_content_bytes};
use flare_proto::common::data_packet::Payload as DataPacketPayload;
use flare_proto::common::{DataPacket, Message, MessagePush, SyncRes};
use prost::Message as ProstMessage;

fn sample_message(seq: u64) -> Message {
    let content = ContentBuilder::text("hello from perf baseline").build();
    Message {
        server_id: format!("srv-{seq}"),
        conversation_id: "c1".to_string(),
        client_msg_id: format!("cli-{seq}"),
        sender_id: "u1".to_string(),
        source: 1,
        seq,
        timestamp: None,
        conversation_type: 1,
        message_type: content.message_type as i32,
        channel_id: "u2".to_string(),
        sender_name: "Alice".to_string(),
        sender_avatar: String::new(),
        content: content.encode(),
        status: 2,
        burn_enabled: false,
        burn_after_read_seconds: None,
        burn_status: 0,
        first_read_at: None,
        burn_at: None,
        burned_at: None,
        offline_push_info: None,
        extra: Default::default(),
        extensions: Default::default(),
    }
}

fn bench_event_bus_publish(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_bus_publish");
    for subscriber_count in [0usize, 1, 10, 100] {
        let bus = EventBus::new();
        let _subscribers = (0..subscriber_count)
            .map(|_| bus.subscribe_raw())
            .collect::<Vec<_>>();

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(subscriber_count),
            &subscriber_count,
            |b, _| {
                b.iter(|| {
                    bus.publish(black_box(SdkEvent::Connection(
                        ConnectionEvent::StateChanged {
                            state: SdkState::Connected,
                        },
                    )));
                });
            },
        );
    }
    group.finish();
}

fn bench_protobuf_codec(c: &mut Criterion) {
    let codec = ProtobufCodec;
    let message_push = MessagePush {
        messages: vec![sample_message(1)],
        notifications: Vec::new(),
    };
    let message_push_payload = message_push.encode_to_vec();
    let sync_packet = DataPacket {
        kind: 2,
        payload: Some(DataPacketPayload::SyncResponse(SyncRes::default())),
    };
    let sync_packet_payload = sync_packet.encode_to_vec();

    let mut group = c.benchmark_group("protobuf_codec_decode");
    group.throughput(Throughput::Bytes(message_push_payload.len() as u64));
    group.bench_function("message_push", |b| {
        b.iter(|| {
            black_box(
                codec
                    .decode_server(black_box(message_push_payload.as_slice()))
                    .expect("message push payload should decode"),
            );
        });
    });
    group.throughput(Throughput::Bytes(sync_packet_payload.len() as u64));
    group.bench_function("sync_data_packet", |b| {
        b.iter(|| {
            black_box(
                codec
                    .decode_server(black_box(sync_packet_payload.as_slice()))
                    .expect("sync data packet should decode"),
            );
        });
    });
    group.finish();
}

fn bench_message_content_decode(c: &mut Criterion) {
    let content = ContentBuilder::text("decode baseline text payload")
        .mention_user("u2", 7, 8)
        .build()
        .encode();

    c.benchmark_group("message_content")
        .throughput(Throughput::Bytes(content.len() as u64))
        .bench_function("decode_text", |b| {
            b.iter(|| {
                black_box(
                    decode_content_bytes(black_box(content.as_slice()))
                        .expect("text message content should decode"),
                );
            });
        });
}

criterion_group!(
    perf_baseline,
    bench_event_bus_publish,
    bench_protobuf_codec,
    bench_message_content_decode
);
criterion_main!(perf_baseline);
