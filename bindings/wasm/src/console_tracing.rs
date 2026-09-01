//! 把核心的 `tracing` 事件转发到浏览器控制台。
//!
//! 之前 wasm 绑定只装了 `console_error_panic_hook`，只有 panic 才看得见。
//! 结果是核心里所有 `warn!`/`error!`（发送队列、传输层、同步）在浏览器里
//! **完全不可见**——排查线上问题时只能靠黑盒推断，代价极高。
//!
//! 这里不引入新依赖（`tracing-wasm`/`tracing-subscriber` 在 wasm 上都要额外裁剪），
//! 直接实现一个最小 `Subscriber`：只处理事件，span 相关方法留空。
//! 默认阈值 WARN，可用 `flareSetLogLevel("info"|"debug"|...)` 在运行时调高。

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Metadata, Subscriber, span};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = debug)]
    fn console_debug(msg: &str);
    #[wasm_bindgen(js_namespace = console, js_name = info)]
    fn console_info(msg: &str);
    #[wasm_bindgen(js_namespace = console, js_name = warn)]
    fn console_warn(msg: &str);
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(msg: &str);
}

/// 0=off 1=error 2=warn 3=info 4=debug 5=trace
static LEVEL: AtomicUsize = AtomicUsize::new(2);

fn level_rank(level: &Level) -> usize {
    match *level {
        Level::ERROR => 1,
        Level::WARN => 2,
        Level::INFO => 3,
        Level::DEBUG => 4,
        Level::TRACE => 5,
    }
}

/// 运行时调整核心日志级别，便于线上排查而不必重新构建 WASM。
#[wasm_bindgen(js_name = flareSetLogLevel)]
pub fn flare_set_log_level(level: &str) {
    let rank = match level.trim().to_ascii_lowercase().as_str() {
        "off" | "none" => 0,
        "error" => 1,
        "warn" | "warning" => 2,
        "info" => 3,
        "debug" => 4,
        "trace" => 5,
        _ => 2,
    };
    LEVEL.store(rank, Ordering::Relaxed);
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: String,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
            // Debug 格式化的字符串带引号，去掉更易读
            if self.message.len() >= 2
                && self.message.starts_with('"')
                && self.message.ends_with('"')
            {
                self.message = self.message[1..self.message.len() - 1].to_string();
            }
        } else {
            if !self.fields.is_empty() {
                self.fields.push(' ');
            }
            self.fields
                .push_str(&format!("{}={value:?}", field.name()));
        }
    }
}

struct ConsoleSubscriber;

impl Subscriber for ConsoleSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        level_rank(metadata.level()) <= LEVEL.load(Ordering::Relaxed)
    }

    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        // 不做 span 跟踪：这里只为把事件送到控制台，span 层次对排查帮助有限，
        // 而维护 span 栈需要线程本地状态，在 wasm 上得不偿失。
        span::Id::from_u64(1)
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}
    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}
    fn enter(&self, _span: &span::Id) {}
    fn exit(&self, _span: &span::Id) {}

    fn event(&self, event: &Event<'_>) {
        let metadata = event.metadata();
        if !self.enabled(metadata) {
            return;
        }
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let line = if visitor.fields.is_empty() {
            format!("[flare-core] {} {}", metadata.target(), visitor.message)
        } else {
            format!(
                "[flare-core] {} {} | {}",
                metadata.target(),
                visitor.message,
                visitor.fields
            )
        };
        match *metadata.level() {
            Level::ERROR => console_error(&line),
            Level::WARN => console_warn(&line),
            Level::INFO => console_info(&line),
            _ => console_debug(&line),
        }
    }
}

/// 幂等安装；重复调用后续会失败但无害。
pub fn install() {
    let _ = tracing::subscriber::set_global_default(ConsoleSubscriber);
}
