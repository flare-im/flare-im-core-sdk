# 接入你自己的 E2EE 实现

Flare 的核心**不实现具体密码学算法**，只提供稳定的档位契约与 fail-closed 管线。
算法由你通过 `ContentCodec` 接入。

这不是偷懒，是刻意的：

- 密码学实现一旦发布就极难更换 —— 用户数据用旧算法加密着，迁移要跨版本、
  跨设备、跨历史消息。锁进核心等于把项目绑死在一个算法上
- 不同场景要的根本不是同一种 E2EE：消费级要双棘轮（前向安全），大群要
  sender-key 或 MLS，金融政务要国密与硬件密钥
- **企业合规要「可审计解密」，消费级 E2EE 要「谁也解不开」—— 这两个诉求
  在密码学上是对立的**，一套实现不可能同时满足

如果你有自己的密码学要求（金融、政务常见），这个设计正是你要的。

## 三层契约

```
ConversationEncryptionPolicy   决定这个会话用哪个档位
        ↓
ContentEncryptionInterceptor   E2e 档位时拦截发送
        ↓
ContentCodec                   ← 你实现这个
```

档位有三档：

| 档位 | 行为 |
|---|---|
| `None` | 不加密 |
| `Transport` | 仅依赖传输层加密（TLS/QUIC），内容不改写 |
| `E2e` | 发送前用你的 codec 把明文封装成密文占位符 |

## 实现 ContentCodec

```rust
use flare_im_core_sdk::{ContentCodec, MessageContent};

struct MyCodec { /* 你的密钥材料 */ }

impl ContentCodec for MyCodec {
    fn content_type(&self) -> &str { "myorg.e2ee.v1" }

    fn encode(&self, plain: &MessageContent) -> Result<Vec<u8>, CodecError> {
        // 用你的算法加密。核心不关心你怎么做。
    }

    fn decode(&self, cipher: &[u8]) -> Result<MessageContent, CodecError> {
        // 解密失败**必须返回 Err**，不要返回可疑明文 ——
        // 管线会据此产出 decrypt_failed 占位符而不是展示垃圾内容。
    }
}
```

装配：

```rust
let client = FlareImClient::builder()
    .conversation_encryption(
        Arc::new(MyPolicyResolver),   // 决定哪些会话走 E2e
        Arc::new(MyCodec::new(keys)),
    )
    .build()?;
```

## 核心已经替你做好的

**发送管线 fail-closed**：`E2e` 档位下 codec 返回错误时，消息**不会以明文发出**。
这条是最容易写错的地方 —— 很多实现在加密失败时降级成明文发送，等于没有加密。

**协议侧占位符**：`PlaceholderContent` 已定义 `e2e_placeholder`、`decrypt_failed`
等 reason 与 `fallback_text`，各端 UI 已能渲染「[加密消息]」「[解密失败]」。

**推送侧不泄露**：离线推送识别 E2EE 占位符后改用通用文案，
**不会把密文或原文推到通知栏**（`flare-push/worker` 的 `push_display`）。

**密钥模型**：`E2eeIdentityKey` / `E2eePreKeyBundle` / `E2eeSessionDescriptor`
与 `E2eeKeyManager` trait 已定义，你可以直接用，也可以只用 `ContentCodec` 而
自行管理密钥。

## 你必须自己做的

### 1. 密钥分发

核心**不提供** prekey bundle 的服务端存取接口。你需要自己搭一条通道让设备
交换公钥材料 —— 可以走你已有的用户系统，也可以用 `flare-im-hooks` 挂在
消息管线上。

### 2. 多设备同步

新设备登录后如何读到历史加密消息，取决于你的密钥方案。核心不做假设。

### 3. 密钥指纹校验

**这一条不做，E2EE 就只是营销话术。** 用户必须能验证「我在和谁通话」，
否则中间人攻击无法察觉 —— 服务端替换掉公钥，双方都不会发现。

至少要提供：指纹展示、二维码扫描或口头核对、以及指纹变更时的显著告警。

### 4. 解密失败的降级

密钥不同步时用户会看到一片「[解密失败]」。协议侧 reason 已经有了，
但你要设计重试与恢复路径 —— **无从恢复的解密失败，比收不到消息更糟**。

## 可以参考的实现

| 库 | 适用 |
|---|---|
| [vodozemac](https://github.com/matrix-org/vodozemac) | Olm/Megolm（Rust，经审计）。单聊双棘轮 + 群 Megolm |
| [OpenMLS](https://github.com/openmls/openmls) | MLS（RFC 9420）。单聊与大群统一，标准化程度高 |
| 国密方案 | 金融/政务合规场景，通常需配合硬件密钥与密钥托管 |

## 现状说明

**核心目前不附带任何生产级 `ContentCodec` 实现** —— 仓库里唯一的
`PrefixCodec` 位于测试模块，只做前缀标记，不是密码学实现，**不要用于生产**。

详细评估见工作区 `docs/roadmap/e2ee-assessment.md`。
