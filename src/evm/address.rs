use super::EvmProvider;
use alloy::{primitives::Address, providers::Provider as _, sol, transports::TransportError};
use std::future::IntoFuture;

sol! {
    #[sol(rpc)]
    interface IERC20Metadata {
        function name() external view returns (string);
        function symbol() external view returns (string);
        function decimals() external view returns (uint8);
    }
}

#[derive(Debug, Clone)]
pub enum AddressInfo {
    EOA,
    Erc20Contract(Erc20Contract),
    OtherContract,
}

impl AddressInfo {
    pub fn is_contract(&self) -> bool {
        match self {
            AddressInfo::EOA => false,
            _ => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Erc20Contract {
    pub symbol: Option<Box<str>>,
    pub name: Option<Box<str>>,
    pub decimals: u8,
}

impl std::fmt::Display for AddressInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EOA => f.write_str("EOA"),
            Self::OtherContract => f.write_str("Contract"),
            Self::Erc20Contract(token) => {
                let symbol = token.symbol.as_deref().unwrap_or("Unknown");
                match token.name.as_deref() {
                    Some(name) if !name.is_empty() => {
                        write!(
                            f,
                            "Contract: ERC-20 {symbol} ({name}) decimals={}",
                            token.decimals
                        )
                    }
                    _ => write!(f, "Contract: ERC-20 {symbol} decimals={}", token.decimals),
                }
            }
        }
    }
}

/// 通过 provider 拉取地址信息。
///
/// - 没有收到节点响应的传输错误视为 provider 出错，打印日志后返回 [`None`]，调用方不写缓存，等下次事件再试。
/// - 空代码视为 [`AddressInfo::EOA`]。
/// - 节点正常响应但 `decimals` 调用失败（例如合约没有对应方法而回滚）只说明该合约不是标准 ERC-20，归类为 [`AddressInfo::OtherContract`]。
/// - `symbol` / `name` 调用失败只说明没有这两个元数据，结果里对应字段为 [`None`]。
pub async fn fetch_address_info(p: &EvmProvider, addr: Address) -> Option<AddressInfo> {
    let code = match p.get_code_at(addr).await {
        Ok(code) => code,
        Err(err) => {
            tracing::error!(address = %addr, "查询地址代码失败: {err}");
            return None;
        }
    };
    if code.is_empty() {
        return Some(AddressInfo::EOA);
    }

    let token = IERC20Metadata::new(addr, p.clone());
    let decimals = match metadata(token.decimals().call()).await {
        Ok(Some(decimals)) => decimals,
        Ok(None) => return Some(AddressInfo::OtherContract),
        Err(err) => {
            tracing::error!(address = %addr, "查询 ERC-20 decimals 失败: {err}");
            return None;
        }
    };
    let symbol = match metadata(token.symbol().call()).await {
        Ok(symbol) => symbol.map(String::into_boxed_str),
        Err(err) => {
            tracing::error!(address = %addr, "查询 ERC-20 symbol 失败: {err}");
            return None;
        }
    };
    let name = match metadata(token.name().call()).await {
        Ok(name) => name.map(String::into_boxed_str),
        Err(err) => {
            tracing::error!(address = %addr, "查询 ERC-20 name 失败: {err}");
            return None;
        }
    };

    Some(AddressInfo::Erc20Contract(Erc20Contract {
        symbol,
        name,
        decimals,
    }))
}

/// 把合约调用的结果分成三类，避免每个元数据查询都写一遍 match。
///
/// - `Ok(Some(v))`: 调用成功
/// - `Ok(None)`: 节点正常响应但调用回滚
/// - `Err(e)`: 传输错误，调用方应放弃本次查询
async fn metadata<T, F>(fut: F) -> Result<Option<T>, TransportError>
where
    F: IntoFuture<Output = Result<T, alloy::contract::Error>>,
{
    match fut.await {
        Ok(value) => Ok(Some(value)),
        Err(alloy::contract::Error::TransportError(err)) if err.as_error_resp().is_none() => {
            Err(err)
        }
        Err(_) => Ok(None),
    }
}
