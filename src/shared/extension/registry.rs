//! 扩展注册中心
//!
//! 统一管理所有业务扩展点，支持优先级、依赖关系、生命周期管理
//!
//! ## 设计理念
//!
//! 参考 Spring Framework 的 Bean 注册机制和 OSGi 的 Bundle 管理：
//! - **统一注册**: 所有扩展点通过注册中心管理
//! - **优先级支持**: 支持扩展点优先级，高优先级覆盖低优先级
//! - **依赖解析**: 自动解析扩展点依赖关系
//! - **生命周期管理**: 支持扩展点的初始化、运行、销毁
//! - **健康检查**: 定期检查扩展点健康状态

use crate::api::FlareIMClient;
use crate::shared::extension::business::{
    BusinessDomain, BusinessExtensionPoint, ChannelBusinessExtension, GroupBusinessExtension,
    UserBusinessExtension,
};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// 扩展点注册信息
#[derive(Debug, Clone)]
struct ExtensionRegistration {
    /// 扩展点实例
    extension: Arc<dyn BusinessExtensionPoint>,
    /// 业务领域
    domain: BusinessDomain,
    /// 优先级
    priority: u8,
    /// 依赖的业务领域
    dependencies: Vec<BusinessDomain>,
    /// 是否已初始化
    initialized: bool,
    /// 是否健康
    healthy: bool,
}

/// 扩展注册中心
///
/// 统一管理所有业务扩展点
pub struct BusinessExtensionRegistry {
    /// 扩展点注册表（按业务领域分组）
    extensions: Arc<RwLock<HashMap<BusinessDomain, Vec<ExtensionRegistration>>>>,
    /// 扩展点名称映射（用于快速查找）
    name_map: Arc<RwLock<HashMap<String, BusinessDomain>>>,
}

impl BusinessExtensionRegistry {
    /// 创建扩展注册中心
    pub fn new() -> Self {
        Self {
            extensions: Arc::new(RwLock::new(HashMap::new())),
            name_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册业务扩展点
    ///
    /// # 参数
    /// - `extension`: 扩展点实例
    /// - `client`: FlareIMClient 实例（用于初始化）
    ///
    /// # 返回
    /// - `Ok(())`: 注册成功
    /// - `Err`: 注册失败
    ///
    /// # 注意
    /// - 如果同一业务领域已有扩展点，按优先级排序
    /// - 优先级相同时，后注册的排在后面
    pub async fn register<T: BusinessExtensionPoint + 'static>(
        &self,
        extension: Arc<T>,
        client: &FlareIMClient,
    ) -> Result<()> {
        let domain = extension.business_domain();
        let priority = extension.priority();
        let dependencies = extension.dependencies();
        let name = extension.name();

        info!(
            domain = %domain,
            name = name,
            priority = priority,
            "Registering business extension"
        );

        // 检查依赖是否已注册
        if !dependencies.is_empty() {
            let extensions = self.extensions.read().await;
            for dep_domain in &dependencies {
                if !extensions.contains_key(dep_domain) {
                    return Err(anyhow::anyhow!(
                        "Dependency {:?} not found for extension {}",
                        dep_domain,
                        name
                    ));
                }
            }
        }

        // 初始化扩展点
        extension
            .initialize(client)
            .await
            .with_context(|| format!("Failed to initialize extension: {}", name))?;

        // 注册扩展点
        let mut extensions = self.extensions.write().await;
        let registrations = extensions.entry(domain).or_insert_with(Vec::new);

        // 按优先级插入（保持有序）
        let insert_pos = registrations
            .binary_search_by_key(&priority, |reg| reg.priority)
            .unwrap_or_else(|pos| pos);
        registrations.insert(
            insert_pos,
            ExtensionRegistration {
                extension: extension.clone(),
                domain,
                priority,
                dependencies: dependencies.clone(),
                initialized: true,
                healthy: true,
            },
        );

        // 更新名称映射
        let mut name_map = self.name_map.write().await;
        name_map.insert(name.to_string(), domain);

        info!(
            domain = %domain,
            name = name,
            "Business extension registered successfully"
        );

        Ok(())
    }

    /// 注销业务扩展点
    ///
    /// # 参数
    /// - `name`: 扩展点名称
    ///
    /// # 返回
    /// - `Ok(true)`: 注销成功
    /// - `Ok(false)`: 扩展点不存在
    /// - `Err`: 注销失败
    pub async fn unregister(&self, name: &str) -> Result<bool> {
        let name_map = self.name_map.read().await;
        let domain = match name_map.get(name) {
            Some(d) => *d,
            None => return Ok(false),
        };
        drop(name_map);

        let mut extensions = self.extensions.write().await;
        if let Some(registrations) = extensions.get_mut(&domain) {
            // 查找并移除扩展点
            if let Some(pos) = registrations
                .iter()
                .position(|reg| reg.extension.name() == name)
            {
                let reg = registrations.remove(pos);
                // 清理扩展点
                if let Err(e) = reg.extension.cleanup().await {
                    error!(error = %e, name = name, "Failed to cleanup extension");
                }
                info!(name = name, domain = %domain, "Business extension unregistered");
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// 获取用户业务扩展点（按优先级返回第一个）
    ///
    /// # 返回
    /// - `Some(extension)`: 找到扩展点
    /// - `None`: 未找到扩展点
    ///
    /// # 注意
    /// 由于 Rust 类型系统的限制，这里使用类型擦除的方式。
    /// 扩展点需要同时实现 `BusinessExtensionPoint` 和 `UserBusinessExtension`，
    /// 并在注册时提供类型转换函数。
    pub async fn get_user_extension(&self) -> Option<Arc<dyn UserBusinessExtension>> {
        let extensions = self.extensions.read().await;
        if let Some(registrations) = extensions.get(&BusinessDomain::User) {
            for reg in registrations {
                if reg.initialized && reg.healthy {
                    // 使用 Any trait 进行类型擦除和转换
                    // 注意：扩展点必须同时实现 UserBusinessExtension
                    // 这里通过向下转型实现（需要扩展点在注册时提供类型信息）
                    // 暂时返回 None，实际使用时需要通过桥接层实现
                    break;
                }
            }
        }
        None
    }

    /// 获取群组业务扩展点（按优先级返回第一个）
    pub async fn get_group_extension(&self) -> Option<Arc<dyn GroupBusinessExtension>> {
        let extensions = self.extensions.read().await;
        if let Some(registrations) = extensions.get(&BusinessDomain::Group) {
            for reg in registrations {
                if reg.initialized && reg.healthy {
                    break;
                }
            }
        }
        None
    }

    /// 获取频道业务扩展点（按优先级返回第一个）
    pub async fn get_channel_extension(&self) -> Option<Arc<dyn ChannelBusinessExtension>> {
        let extensions = self.extensions.read().await;
        if let Some(registrations) = extensions.get(&BusinessDomain::Channel) {
            for reg in registrations {
                if reg.initialized && reg.healthy {
                    break;
                }
            }
        }
        None
    }

    /// 获取指定业务领域的扩展点列表（按优先级排序）
    pub async fn get_extensions_by_domain(
        &self,
        domain: BusinessDomain,
    ) -> Vec<Arc<dyn BusinessExtensionPoint>> {
        let extensions = self.extensions.read().await;
        if let Some(registrations) = extensions.get(&domain) {
            registrations
                .iter()
                .filter(|reg| reg.initialized && reg.healthy)
                .map(|reg| reg.extension.clone())
                .collect()
        } else {
            vec![]
        }
    }

    /// 获取所有已注册的扩展点
    pub async fn list_extensions(&self) -> Vec<ExtensionInfo> {
        let extensions = self.extensions.read().await;
        let mut result = Vec::new();

        for (domain, registrations) in extensions.iter() {
            for reg in registrations {
                result.push(ExtensionInfo {
                    name: reg.extension.name().to_string(),
                    domain: *domain,
                    priority: reg.priority,
                    initialized: reg.initialized,
                    healthy: reg.healthy,
                });
            }
        }

        result
    }

    /// 健康检查
    ///
    /// 检查所有扩展点的健康状态
    pub async fn health_check(&self) -> Result<HealthCheckResult> {
        let extensions = self.extensions.read().await;
        let mut healthy_count = 0;
        let mut unhealthy_count = 0;
        let mut errors = Vec::new();

        for (domain, registrations) in extensions.iter() {
            for reg in registrations {
                match reg.extension.health_check().await {
                    Ok(true) => {
                        healthy_count += 1;
                    }
                    Ok(false) | Err(_) => {
                        unhealthy_count += 1;
                        errors.push(format!(
                            "Extension {} (domain: {:?}) is unhealthy",
                            reg.extension.name(),
                            domain
                        ));
                    }
                }
            }
        }

        Ok(HealthCheckResult {
            healthy_count,
            unhealthy_count,
            errors,
        })
    }
}

/// 扩展点信息
#[derive(Debug, Clone)]
pub struct ExtensionInfo {
    /// 扩展点名称
    pub name: String,
    /// 业务领域
    pub domain: BusinessDomain,
    /// 优先级
    pub priority: u8,
    /// 是否已初始化
    pub initialized: bool,
    /// 是否健康
    pub healthy: bool,
}

/// 健康检查结果
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    /// 健康的扩展点数量
    pub healthy_count: usize,
    /// 不健康的扩展点数量
    pub unhealthy_count: usize,
    /// 错误信息列表
    pub errors: Vec<String>,
}

impl Default for BusinessExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// 注意：由于 Rust 的类型系统限制，无法在运行时进行 trait object 的类型转换
// 这里需要重新设计，使用类型擦除或枚举类型
// 暂时先提供基础框架，后续可以优化
