//! 统一转换器 Trait 定义

use crate::infrastructure::converter::error::ConversionError;

/// 统一转换器 Trait
/// 
/// 所有转换器必须实现此 Trait，提供双向转换能力
pub trait Converter<From, To>: Send + Sync {
    /// 从源类型转换为目标类型
    fn convert(&self, from: From) -> Result<To, ConversionError>;
    
    /// 从目标类型转换为源类型（反向转换）
    fn convert_back(&self, to: To) -> Result<From, ConversionError>;
}

/// 批量转换支持
pub trait BatchConverter<From, To>: Converter<From, To> {
    /// 批量转换
    fn convert_batch(&self, items: Vec<From>) -> Result<Vec<To>, ConversionError> {
        items
            .into_iter()
            .map(|item| self.convert(item))
            .collect()
    }
    
    /// 批量反向转换
    fn convert_back_batch(&self, items: Vec<To>) -> Result<Vec<From>, ConversionError> {
        items
            .into_iter()
            .map(|item| self.convert_back(item))
            .collect()
    }
}
