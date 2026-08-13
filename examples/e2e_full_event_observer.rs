use flare_im_core_sdk::prelude::*;
use std::time::Duration;

#[path = "common/dev_token.rs"]
mod dev_token;
#[path = "common/diagnose.rs"]
mod diagnose;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // 失败时先把「下一步该做什么」打出来，再原样返回错误。
    // 默认冒泡出的是 Debug 结构体，对第一次跑示例的人几乎没有指导意义。
    diagnose::explain(run().await)
}

async fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let secret = dev_token::require()?;
    let client = IMClient::new();
    client
        .init(Some("e2e-full-event-observer".into()), None)
        .await?;
    let token = IMClient::generate_core_token(CoreTokenConfig {
        secret: secret.clone(),
        issuer: "flare-im-core".to_string(),
        user_id: "full_event_observer".to_string(),
        ttl_secs: 3600,
        device_id: None,
        tenant_id: Some("0".to_string()),
    })?;
    let apis = client
        .login(
            "full_event_observer",
            Some(&token),
            LoginDbKind::Sqlite,
            |_, _| {},
        )
        .await?;

    let _any = client.on_any(|event| {
        println!("sdk event: {:?}", event);
    })?;
    // media-control 侧把 call_id / room_id 当 UUID 解析，传 "call_full" / "room_a"
    // 会被直接拒。示例用固定 UUID 而不是随机值，好让重复运行落在同一个房间、
    // 结果可复现。
    let call_id = "00000000-0000-4000-8000-0000000000c1";
    let room_id = "00000000-0000-4000-8000-0000000000a1";
    // RTC 由 SFU 能力插件提供，而那个插件不在开源仓里。这个示例的主体（全事件面）
    // 到此已经跑完，不该因为一个可选插件缺席就整例变红——那会让只克隆公开仓的人
    // 以为核心链路坏了。
    //
    // 这里**必须自己限时**，不能等服务端报错：能力服务发现不到插件时是按周期重试
    // 发现（实测约每 90 秒一轮），而不是快速失败。靠「错误信息里有没有插件名」来
    // 判断，等于把示例的结束时间交给服务端的重试节奏——CI 上就因此挂到超时。
    const RTC_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
    match tokio::time::timeout(
        RTC_PROBE_TIMEOUT,
        apis.capability_api.rtc_sfu_join_room(
            "example_full",
            call_id,
            room_id,
            Some("speaker"),
            None,
        ),
    )
    .await
    {
        Ok(Ok(result)) => println!("sfu join: {:?}", result.data),
        Ok(Err(e)) if format!("{e:?}").contains("flare-strom-sfu") => {
            println!("sfu join: 跳过 —— SFU 能力插件未运行（RTC 由插件提供，不在开源栈内）");
        }
        Ok(Err(e)) => return Err(e.into()),
        Err(_elapsed) => {
            println!(
                "sfu join: 跳过 —— {}s 内无应答，按 SFU 能力插件未运行处理",
                RTC_PROBE_TIMEOUT.as_secs()
            );
        }
    }
    Ok(())
}
