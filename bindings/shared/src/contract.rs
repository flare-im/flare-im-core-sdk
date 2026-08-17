pub use crate::generated::contract::{
    API_CONTRACT_VERSION, API_OPERATIONS, BINDING_CONTRACT_VERSION, ERROR_CODES,
    ERROR_CONTRACT_VERSION, EVENT_CONTRACT_VERSION, EVENT_DESCRIPTORS, ErrorCode, EventDescriptor,
    MESSAGE_BUILD_OPS, MessageBuildCatalogEntry,
};

pub use crate::generated::contract::ApiOperation;

pub fn find_api_operation(id: &str) -> Option<&'static ApiOperation> {
    API_OPERATIONS.iter().find(|operation| operation.id == id)
}

pub fn find_event_by_id(id: &str) -> Option<&'static EventDescriptor> {
    EVENT_DESCRIPTORS.iter().find(|event| event.id == id)
}

pub fn find_event_by_code(code: i32) -> Option<&'static EventDescriptor> {
    EVENT_DESCRIPTORS.iter().find(|event| event.c_code == code)
}

pub fn find_error_code(name: &str) -> Option<&'static ErrorCode> {
    ERROR_CODES.iter().find(|error| error.name == name)
}

#[cfg(test)]
mod dispatch_contract_tests {
    use super::*;
    use crate::generated::direct_invoke::is_direct_invoke_route;

    /// `API_OPERATIONS` 是**多绑定**注册表：同一个 op 在不同宿主走不同路径
    /// （C FFI 走 `c_symbol` 或 `c_dispatch_op`，tauri 走 `tauri` 命令，
    /// 部分走生成的直接分发表）。所以不能要求「每个 op 都能 JSON invoke」——
    /// 第一版门禁就是这么写的，把 `message.send` 这种明显能用的 op 判成了坏的。
    ///
    /// 真正成立的不变量只有下面这些。
    ///
    /// 每个 op 必须至少有一条宿主可达路径。
    ///
    /// 三个字段全 None 意味着：它列在契约里、生成器会为它产出文档与类型，
    /// 但**任何宿主都调不到它**。这种 op 不会让编译失败、不会让单测变红，
    /// 只会让照着契约集成的人拿到一个不存在的能力。
    #[test]
    fn every_operation_is_reachable_from_some_binding() {
        let orphans: Vec<&str> = API_OPERATIONS
            .iter()
            .filter(|op| op.c_symbol.is_none() && op.c_dispatch_op.is_none() && op.tauri.is_none())
            .map(|op| op.id)
            .collect();

        assert!(
            orphans.is_empty(),
            "以下 op 列在契约里，但没有任何宿主绑定，谁都调不到：{orphans:?}"
        );
    }

    /// 每个 op 必须指明它由哪个核心方法实现。
    ///
    /// `core: None` 说明契约与实现之间断了链：生成器无从知道该调什么，
    /// 而契约文档仍然把它当成一个可用能力对外展示。
    #[test]
    fn every_operation_declares_its_core_method() {
        let dangling: Vec<&str> = API_OPERATIONS
            .iter()
            .filter(|op| op.core.is_none())
            .map(|op| op.id)
            .collect();

        assert!(dangling.is_empty(), "以下 op 没有声明核心实现方法：{dangling:?}");
    }

    /// 声明了 `c_dispatch_op` 的，那个**子操作名**必须真的能被对应模块分发。
    ///
    /// 语义容易看错，写坏过两版门禁，记在这里：`c_dispatch_op` 不是顶层路由，
    /// 而是模块内的子操作名——`message.send_no_oss` 的 `c_dispatch_op` 是
    /// `"send_no_oss"`，由 `flare_message_dispatch_json(op="send_no_oss")` 分发。
    /// 拿它当路由去查 `is_direct_invoke_route` 会把 67 个正常 op 判成坏的。
    ///
    /// 权威判据就是原生端自己用的那几个 `is_*_operation`：C FFI 入口先用它放行，
    /// 不认识就直接返回 `binding_operation_not_supported`。契约声明了而判据不认，
    /// 意味着 iOS / Android 调这个 op 必失败——且只有真机跑到才会发现。
    #[test]
    fn declared_c_dispatch_ops_are_recognized_by_their_module() {
        use crate::generated::dispatch::{
            capability::is_capability_operation, conversation::is_conversation_operation,
            media::is_media_operation, message::is_message_operation,
            message_build::is_message_build_operation,
        };

        let unrecognized: Vec<String> = API_OPERATIONS
            .iter()
            .filter_map(|op| op.c_dispatch_op.map(|sub| (op.id, op.module, sub)))
            .filter(|(_, module, sub)| {
                let recognized = match *module {
                    "message" => is_message_operation(sub),
                    "message_builder" => is_message_build_operation(sub),
                    "conversation" => is_conversation_operation(sub),
                    "media" => is_media_operation(sub),
                    "capability" => is_capability_operation(sub),
                    // 没有专属判据的模块不在本门禁范围内——与其用一个猜的默认值
                    // 制造假阴性或假阳性，不如明确跳过。
                    _ => true,
                };
                !recognized
            })
            .map(|(id, module, sub)| format!("{id} (module={module}) → 子操作 {sub:?}"))
            .collect();

        assert!(
            unrecognized.is_empty(),
            "以下 op 声明了 c_dispatch_op，但所属模块的分发判据不认识它，\
             原生端调用会拿到 binding_operation_not_supported：\n  {}",
            unrecognized.join("\n  ")
        );
    }

    /// op id 不得重复：重复意味着后一条永远不会被命中。
    #[test]
    fn operation_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        let dups: Vec<_> = API_OPERATIONS
            .iter()
            .filter(|op| !seen.insert(op.id))
            .map(|op| op.id)
            .collect();
        assert!(dups.is_empty(), "重复的 op id：{dups:?}");
    }
}
